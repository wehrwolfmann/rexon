//! Golden-сверка порта `qt_extensions.rs` с оригиналом —
//! `rexon/qt_extensions/window.py` в версии до переноса.
//!
//! Эталон снят прогоном ЖИВОЙ вкладки под offscreen-Qt на подставном доме
//! (`golden/qt_extensions_cases.py` → `golden/qt_extensions_cases.json`):
//!   * дерево-фикстура из семи каталогов, включая два заведомо негодных
//!     (без манифеста и с битым JSON) — их вкладка обязана пропустить;
//!   * ПОЛНОЕ дерево подписей собранного окна в обоих режимах показа и на
//!     пустом каталоге;
//!   * карточка и подробный разбор каждого расширения по отдельности;
//!   * кнопки фильтров, строка статистики, заголовок «Мои расширения»,
//!     пустые состояния, поиск по шести запросам;
//!   * строка состояния после каждого исхода, включая обрезку длинной
//!     ошибки на 200 символах.
//!
//! Санкционированные расхождения (починки оригинала, правило #16) в голдене
//! НЕ отражены — там записано поведение Python как есть; каждая проверяется
//! отдельным тестом внизу файла:
//!   1. имя, автор, описание, теги и README теперь экранируются: у
//!      оригинала `<разметкой>` в имени и `<изменений>` в README ИСЧЕЗАЛИ с
//!      витрины — это видно прямо в эталоне (`body_plain` разбора
//!      `theme-dark` потерял слово);
//!   2. порядок карточек больше не зависит от порядка файловой системы:
//!      каталоги перебираются отсортированными;
//!   3. пустое состояние одно на все ветки — обработчик фильтра больше не
//!      прячет подсказку «Расширений ещё нет».
//!
//! ГЕЙТ «голден обязан ловить регрессию» проверен мутацией: шесть точечных
//! поломок реализации (в статистике потерян разделитель `  •  `; фильтры
//! собираются по `valid_types()` вместо порядка подписей; `my_header`
//! показывает счётчик всегда; группы списка сортируются по алфавиту;
//! обрезка ошибки считает байты вместо символов; в строке типа переставлены
//! значок и подпись) — каждая красит тесты.
//!
//! ⚠️ Ради второй из них в фикстуру добавлены `theme-kde-global` и
//! `theme-kvantum`: у этих двух типов порядок в таблице подписей и в
//! `valid_types()` РАЗНЫЙ, и без них сверка порядка не ловила ничего.


mod i18n_pin;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rexon_core::qt_extensions as qe;
use serde_json::{Map, Value};

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_extensions_cases.json"
    );
    let text = fs::read_to_string(path).expect("нет снятого эталона qt_extensions_cases.json");
    serde_json::from_str(&text).expect("эталон не разбирается как JSON")
}

/// Построить дерево-фикстуру во временном каталоге ровно так же, как это
/// делал снимок: описание лежит в самом эталоне, второй его копии тут нет.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str, g: &Value) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rexon-qt-ext-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let ext_root = root.join(".config/rexon/extensions");
        fs::create_dir_all(&ext_root).expect("не создать дерево-фикстуру");
        for (name, spec) in g["fixture"].as_object().expect("fixture — объект") {
            let dir = ext_root.join(name);
            fs::create_dir_all(&dir).unwrap();
            if let Some(manifest) = spec.get("manifest") {
                fs::write(
                    dir.join("manifest.json"),
                    serde_json::to_string_pretty(manifest).unwrap(),
                )
                .unwrap();
            }
            if let Some(raw) = spec.get("raw_manifest").and_then(Value::as_str) {
                fs::write(dir.join("manifest.json"), raw).unwrap();
            }
            if let Some(files) = spec.get("files").and_then(Value::as_object) {
                for (fname, content) in files {
                    fs::write(dir.join(fname), content.as_str().unwrap_or("")).unwrap();
                }
            }
        }
        fs::write(
            root.join(".config/rexon/extensions_state.json"),
            serde_json::to_string_pretty(&g["state"]).unwrap(),
        )
        .unwrap();
        Fixture { root }
    }

    fn ext_dir(&self) -> PathBuf {
        self.root.join(".config/rexon/extensions")
    }

    fn state(&self) -> Map<String, Value> {
        let text = fs::read_to_string(self.root.join(".config/rexon/extensions_state.json")).ok();
        rexon_core::ext_manager::load_state(text.as_deref())
    }

    fn views(&self) -> Vec<qe::ExtView> {
        let state = self.state();
        qe::installed_in(&self.ext_dir(), &state)
            .iter()
            .map(qe::ExtView::from_extension)
            .collect()
    }

    /// Путь с подставленным `{ROOT}` — как нормализовал снимок.
    fn norm(&self, text: &str) -> String {
        text.replace(&self.root.to_string_lossy().to_string(), "{ROOT}")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Расширение по идентификатору.
fn by_id<'a>(views: &'a [qe::ExtView], id: &str) -> &'a qe::ExtView {
    views
        .iter()
        .find(|v| v.ext_id == id)
        .unwrap_or_else(|| panic!("в фикстуре нет расширения «{id}»"))
}

/// Подписи (`kind == "label"`) из снятого дерева.
fn labels(tree: &Value) -> Vec<String> {
    tree.as_array()
        .expect("дерево — массив")
        .iter()
        .filter(|e| e["kind"] == "label")
        .map(|e| e["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

// ── состав и порядок ────────────────────────────────────────────────────

#[test]
fn discovery_matches_python() {
    let g = golden();
    let fx = Fixture::new("discover", &g);
    let views = fx.views();

    let mine: BTreeMap<String, (String, bool)> = views
        .iter()
        .map(|v| (v.ext_id.clone(), (v.ext_type.clone(), v.enabled)))
        .collect();
    let theirs: BTreeMap<String, (String, bool)> = g["discovered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["id"].as_str().unwrap().to_string(),
                (
                    e["type"].as_str().unwrap().to_string(),
                    e["enabled"].as_bool().unwrap(),
                ),
            )
        })
        .collect();
    assert_eq!(mine, theirs, "состав установленных разошёлся с оригиналом");
    // Каталоги без манифеста и с битым JSON обязаны быть пропущены, а не
    // уронить вкладку.
    assert!(!mine.contains_key("no-manifest"));
    assert!(!mine.contains_key("bad-json"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn extension_fields_match_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("fields", &g);
    let views = fx.views();
    for e in g["discovered"].as_array().unwrap() {
        let id = e["id"].as_str().unwrap();
        let v = by_id(&views, id);
        assert_eq!(v.name, e["name"].as_str().unwrap(), "имя «{id}»");
        assert_eq!(v.version, e["version"].as_str().unwrap(), "версия «{id}»");
        assert_eq!(v.author, e["author"].as_str().unwrap(), "автор «{id}»");
        assert_eq!(
            v.description,
            e["description"].as_str().unwrap(),
            "описание «{id}»"
        );
        assert_eq!(
            v.type_label,
            e["type_label"].as_str().unwrap(),
            "подпись типа «{id}»"
        );
        assert_eq!(
            v.type_icon,
            e["type_icon"].as_str().unwrap(),
            "значок типа «{id}»"
        );
        let tags: Vec<String> = e["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap().to_string())
            .collect();
        assert_eq!(v.tags, tags, "теги «{id}»");
        let de: Vec<String> = e["de_support"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap().to_string())
            .collect();
        assert_eq!(v.de_support, de, "рабочие столы «{id}»");
        assert_eq!(fx.norm(&v.dir), e["dir"].as_str().unwrap(), "каталог «{id}»");
        assert_eq!(
            fx.norm(&v.preview_path),
            e["preview_path"].as_str().unwrap(),
            "превью «{id}»"
        );
        assert_eq!(
            fx.norm(&v.icon_path),
            e["icon_path"].as_str().unwrap(),
            "значок «{id}»"
        );
        let readme = qe::read_readme(v).unwrap_or_default();
        assert_eq!(readme, e["readme"].as_str().unwrap(), "README «{id}»");
    }
}

// ── постоянные вкладки ──────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn constants_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    assert_eq!(qe::CARD_W, g["card_w"].as_i64().unwrap() as i32);
    assert_eq!(qe::CARD_TEXT_W, g["card_text_w"].as_i64().unwrap() as i32);
    assert_eq!(qe::PREVIEW_H, g["preview_h"].as_i64().unwrap() as i32);
    assert_eq!(qe::window_title(), g["window_title"].as_str().unwrap());
    let size = g["window_min_size"].as_array().unwrap();
    assert_eq!(qe::WINDOW_MIN_SIZE.0 as i64, size[0].as_i64().unwrap());
    assert_eq!(qe::WINDOW_MIN_SIZE.1 as i64, size[1].as_i64().unwrap());

    let mut mine: Vec<&str> = qe::DROP_EXTENSIONS.to_vec();
    mine.sort_unstable();
    let theirs: Vec<String> = g["drop_extensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(mine, theirs, "набор принимаемых файлов разошёлся");
}

#[test]
fn drop_check_is_case_insensitive() {
    assert!(qe::is_supported_drop("/tmp/тема.RXN"));
    assert!(qe::is_supported_drop("/tmp/a.themepack"));
    assert!(!qe::is_supported_drop("/tmp/a.zip"));
    assert!(!qe::is_supported_drop("/tmp/безрасширения"));
}

// ── статистика, заголовки, фильтры ──────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn stats_line_matches_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("stats", &g);
    let views = fx.views();
    let stats = qe::stats(&views);
    assert_eq!(qe::stats_line(&stats), g["stats_text"].as_str().unwrap());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn filters_match_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("filters", &g);
    let views = fx.views();
    let items = qe::filter_items(&qe::installed_types(&views));
    let mine: Vec<(String, String)> = items
        .iter()
        .map(|i| (i.id.clone(), i.label.clone()))
        .collect();
    let theirs: Vec<(String, String)> = g["filters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["id"].as_str().unwrap().to_string(),
                f["text"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(mine, theirs, "кнопки фильтров разошлись с оригиналом");
}

#[test]
fn filter_resets_when_its_button_is_gone() {
    let items = qe::filter_items(&["plugin".to_string()]);
    assert_eq!(qe::normalize_filter("plugin", &items), "plugin");
    // Плагин удалили — кнопки больше нет, выбор обязан сброситься.
    let items = qe::filter_items(&[]);
    assert_eq!(qe::normalize_filter("plugin", &items), qe::FILTER_ALL_ID);
    assert_eq!(items.len(), 1, "остаётся только «Все»");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn my_header_matches_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let total = g["discovered"].as_array().unwrap().len();
    assert_eq!(
        qe::my_header(total, total).unwrap(),
        g["my_label"].as_str().unwrap()
    );
    assert_eq!(
        qe::my_header(1, total).unwrap(),
        g["filter_plugin"]["my_label"].as_str().unwrap()
    );
    assert_eq!(qe::my_header(0, 0), None, "над пустотой заголовка нет");
}

// ── карточка ────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn cards_match_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("cards", &g);
    let views = fx.views();
    for (id, tree) in g["cards"].as_object().unwrap() {
        let v = by_id(&views, id);
        let c = qe::card(v);
        let their = labels(tree);
        // Первая подпись — значок-заглушка превью (в снимке картинки
        // ненастоящие, поэтому у всех карточек виден значок типа).
        assert_eq!(their[0], c.placeholder, "заглушка превью «{id}»");
        assert_eq!(their[1], c.type_line, "строка типа «{id}»");
        // Имя в оригинале обрезано по метрикам шрифта — сверяем начало.
        let shown = their[2].trim_end_matches('…');
        assert!(
            c.name.starts_with(shown),
            "имя «{id}»: витрина показала «{shown}», ядро отдало «{}»",
            c.name
        );
        let mut rest = their[3..].to_vec();
        if !c.author_line.is_empty() {
            assert_eq!(rest.remove(0), c.author_line, "автор «{id}»");
        }
        if c.has_toggle {
            assert_eq!(rest.remove(0), c.state_text, "статус «{id}»");
        }
        assert!(rest.is_empty(), "лишние подписи карточки «{id}»: {rest:?}");

        // Кнопки: выключатель (если есть) и удаление.
        let buttons: Vec<(String, String)> = tree
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "button")
            .map(|e| {
                (
                    e["text"].as_str().unwrap().to_string(),
                    e["tooltip"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let mut expected: Vec<(String, String)> = Vec::new();
        if c.has_toggle {
            expected.push((c.toggle_text.clone(), String::new()));
        }
        expected.push((
            qe::UNINSTALL_ICON.to_string(),
            qe::uninstall_tooltip(),
        ));
        assert_eq!(buttons, expected, "кнопки карточки «{id}»");
    }
}

#[test]
fn toggle_only_for_scripted_types() {
    assert!(qe::is_toggleable("plugin"));
    assert!(qe::is_toggleable("desktop-tool"));
    assert!(!qe::is_toggleable("theme-gtk"));
    assert!(!qe::is_toggleable("wallpaper-pack"));
}

// ── режим «список» ──────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn list_groups_and_rows_match_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("list", &g);
    let views = fx.views();
    let filtered = qe::apply_filters(&views, qe::FILTER_ALL_ID, "");
    let groups = qe::list_groups(&filtered);

    // Собираем ожидаемую последовательность подписей режима «список»:
    // заголовок группы, затем значок, название и подпись каждой строки.
    let mut mine: Vec<String> = Vec::new();
    for grp in &groups {
        mine.push(grp.header_html.clone());
        for row_index in &grp.rows {
            let row = qe::list_row(filtered[*row_index]);
            mine.push(row.icon);
            mine.push(row.title_html);
            if !row.subtitle.is_empty() {
                mine.push(row.subtitle);
            }
        }
    }

    // В снимке дереву режима «список» предшествуют осиротевшие карточки
    // прежнего режима (Qt удаляет их отложенно), поэтому берём хвост от
    // первого заголовка группы.
    let their_all = labels(&g["tree_list"]);
    let start = their_all
        .iter()
        .position(|t| t == &groups[0].header_html)
        .expect("в снимке нет первого заголовка группы");
    // Хвост после последней строки списка — подвал страницы («⬇ Установить»
    // и примечание о планах); он сверяется отдельно, в page_chrome.
    let their: Vec<String> = their_all[start..start + mine.len()].to_vec();

    // Оригинал не экранировал разметку и обрезал длинные имена — сверяем по
    // «расшитому» виду: снимаем экранирование со своих строк.
    let unescape = |s: &str| {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    };
    let mine_raw: Vec<String> = mine.iter().map(|s| unescape(s)).collect();
    assert_eq!(
        mine_raw, their,
        "дерево режима «список» разошлось с оригиналом"
    );
}

#[test]
fn list_group_order_follows_label_table() {
    let g = golden();
    let fx = Fixture::new("grouporder", &g);
    let views = fx.views();
    let filtered = qe::apply_filters(&views, qe::FILTER_ALL_ID, "");
    let order: Vec<String> = qe::list_groups(&filtered)
        .iter()
        .map(|grp| grp.ext_type.clone())
        .collect();
    // Ожидаемый порядок берётся из СНИМКА: там это порядок кнопок фильтров,
    // который вкладка строила той же таблицей подписей. Списка-копии здесь
    // нет — иначе он разъехался бы с фикстурой при первом же её пополнении.
    let expected: Vec<String> = g["filters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .filter(|id| id != qe::FILTER_ALL_ID)
        .collect();
    assert_eq!(
        order, expected,
        "порядок групп обязан идти таблицей подписей, а не алфавитом"
    );
}

// ── подробный разбор ────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn details_match_python() {
    // Подписи типов — английские `msgid`; эталон помнит русский (`po/ru.po`).
    let _lang = i18n_pin::ru();
    let g = golden();
    let fx = Fixture::new("details", &g);
    let views = fx.views();
    let home = fx.root.to_string_lossy().to_string();
    for (id, snap) in g["details"].as_object().unwrap() {
        let v = by_id(&views, id);
        let readme = qe::read_readme(v);
        let d = qe::detail(v, readme.as_deref(), &home);
        assert_eq!(
            d.window_title,
            snap["title"].as_str().unwrap(),
            "заголовок разбора «{id}»"
        );
        let size = snap["min_size"].as_array().unwrap();
        assert_eq!(qe::DETAIL_MIN_SIZE.0 as i64, size[0].as_i64().unwrap());
        assert_eq!(qe::DETAIL_MIN_SIZE.1 as i64, size[1].as_i64().unwrap());

        let their = labels(&snap["tree"]);
        let unescape = |s: &str| {
            s.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&amp;", "&")
        };
        let mut mine = vec![
            unescape(&d.heading_html),
            d.meta_line.clone(),
        ];
        if !d.tags_line.is_empty() {
            mine.push(d.tags_line.clone());
        }
        mine.push(unescape(&d.tech_html));
        mine.push(unescape(&fx.norm(&d.path_html).replace("{ROOT}", "~")));
        assert_eq!(mine, their, "подписи разбора «{id}»");

        let buttons: Vec<String> = snap["tree"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "button")
            .map(|e| e["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            buttons,
            vec![
                qe::detail_open_folder_text(),
                qe::detail_uninstall_text(),
                qe::detail_close_text(),
            ],
            "кнопки разбора «{id}»"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn detail_body_falls_back_like_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let fx = Fixture::new("body", &g);
    let views = fx.views();
    // README есть — показывается он.
    let v = by_id(&views, "theme-dark");
    let d = qe::detail(v, qe::read_readme(v).as_deref(), "");
    assert!(d.body_html.starts_with("<pre"), "README обязан идти <pre>");
    // Длинные строки README переносятся, а не уходят за край окна.
    assert!(d.body_html.contains("white-space: pre-wrap"), "{}", d.body_html);
    assert!(
        d.body_html.contains("&lt;изменений&gt;"),
        "слово в угловых скобках обязано уцелеть: {}",
        d.body_html
    );
    // README нет, описание есть.
    let v = by_id(&views, "my-plugin");
    let d = qe::detail(v, qe::read_readme(v).as_deref(), "");
    assert!(d.body_html.starts_with("<p>"));
    // Ни README, ни описания.
    let v = by_id(&views, "tool-x");
    let d = qe::detail(v, qe::read_readme(v).as_deref(), "");
    assert_eq!(d.body_html, qe::detail_no_description());
}

#[test]
fn detail_tags_are_capped_like_python() {
    let g = golden();
    let fx = Fixture::new("tags", &g);
    let views = fx.views();
    let v = by_id(&views, "my-plugin");
    let d = qe::detail(v, None, "");
    assert_eq!(
        d.tags_line.split("  ").count(),
        qe::DETAIL_TAGS_LIMIT,
        "показываем ровно двадцать тегов, как оригинал"
    );
}

// ── справка «Создать своё» ──────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn create_dialog_matches_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let dlg = &g["create_dialog"];
    assert_eq!(qe::create_title(), dlg["title"].as_str().unwrap());
    let size = dlg["min_size"].as_array().unwrap();
    assert_eq!(qe::CREATE_MIN_SIZE.0 as i64, size[0].as_i64().unwrap());
    assert_eq!(qe::CREATE_MIN_SIZE.1 as i64, size[1].as_i64().unwrap());
    let their = labels(&dlg["tree"]);
    assert!(
        their.contains(&qe::create_header()),
        "заголовок справки разошёлся: {their:?}"
    );
    let buttons: Vec<String> = dlg["tree"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "button")
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(buttons, vec![qe::create_close_text()]);

    // Текст справки: сверяем по видимому виду (снимок снят с QTextBrowser,
    // то есть уже без разметки).
    let plain = dlg["body_plain"].as_str().unwrap();
    for probe in [
        "manifest.json",
        "format_version: 3",
        "zip -r ../my-extension.rxn .",
        "Моя крутая тема",
        "desktop-tool",
    ] {
        assert!(
            qe::create_body_html().contains(probe),
            "в справке ядра потерян кусок «{probe}»"
        );
        assert!(
            plain.contains(probe),
            "в справке оригинала нет «{probe}» — фикстура устарела"
        );
    }
}

// ── строка состояния ────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn status_texts_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let s = &g["status"];
    assert_eq!(
        qe::status_installed("Тёмная тема «Ночь»"),
        s["installed"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_install_error("нет такого файла"),
        s["install_error"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_install_error(&"я".repeat(300)),
        s["install_error_long"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_uninstalled("tool-x"),
        s["uninstalled"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_uninstall_error("нет прав"),
        s["uninstall_error"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_uninstall_error(&"я".repeat(300)),
        s["uninstall_error_long"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_installing("пример.rxn"),
        s["installing"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_uninstalling("Утилита X"),
        s["uninstalling"].as_str().unwrap()
    );
    assert_eq!(qe::status_install_busy(), s["install_busy"].as_str().unwrap());
    assert_eq!(
        qe::status_uninstall_busy(),
        s["uninstall_busy"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_toggled(true, "my-plugin"),
        s["toggle_on"].as_str().unwrap()
    );
    assert_eq!(
        qe::status_toggled(false, "my-plugin"),
        s["toggle_off"].as_str().unwrap()
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn long_error_is_clipped_by_characters() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    // ⚠️ 200 СИМВОЛОВ, а не байтов: у оригинала срез `msg[:200]`
    // посимвольный, а байтовый срез на русском тексте разрубил бы символ.
    let text = qe::status_install_error(&"я".repeat(300));
    let tail: String = text.chars().skip("⚠️ Ошибка: ".chars().count()).collect();
    assert_eq!(tail.chars().count(), 200);
}

// ── поиск ───────────────────────────────────────────────────────────────

#[test]
fn search_matches_python() {
    let g = golden();
    let fx = Fixture::new("search", &g);
    let views = fx.views();
    for (query, snap) in g["search"].as_object().unwrap() {
        let found: Vec<String> = qe::apply_filters(&views, qe::FILTER_ALL_ID, query)
            .iter()
            .map(|v| v.ext_id.clone())
            .collect();
        let mut theirs: Vec<String> = snap["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let mut mine = found.clone();
        // Порядок карточек — отдельная починка (см. шапку), здесь важен
        // именно состав найденного.
        mine.sort();
        theirs.sort();
        assert_eq!(mine, theirs, "поиск «{query}» нашёл не то");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn filter_by_type_matches_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let fx = Fixture::new("bytype", &g);
    let views = fx.views();
    let found = qe::apply_filters(&views, "plugin", "");
    assert_eq!(found.len(), 1);
    let _ = &views;
    assert_eq!(found[0].ext_id, "my-plugin");
    assert_eq!(
        qe::my_header(found.len(), views.len()).unwrap(),
        g["filter_plugin"]["my_label"].as_str().unwrap()
    );
}

// ── пустые состояния ────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn empty_states_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    assert_eq!(
        qe::empty_text(5, 5),
        None,
        "при непустой витрине подсказки нет"
    );
    assert_eq!(
        qe::empty_text(0, 0).unwrap(),
        g["empty"]["empty_label"].as_str().unwrap()
    );
    // «Ничего не найдено» снимался поиском по заведомо отсутствующему слову.
    let no_match = g["search"]["нетакого"]["empty"].as_str().unwrap();
    assert_eq!(qe::empty_text(0, 5).unwrap(), no_match);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn empty_catalog_has_only_all_filter() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let theirs = g["empty"]["filters"].as_array().unwrap();
    let items = qe::filter_items(&[]);
    assert_eq!(items.len(), theirs.len());
    assert_eq!(items[0].id, qe::FILTER_ALL_ID);
    assert_eq!(items[0].label, qe::filter_all_label());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn empty_catalog_stats_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let stats = qe::stats(&[]);
    assert_eq!(
        qe::stats_line(&stats),
        g["empty"]["stats_text"].as_str().unwrap()
    );
}

// ── блок «Установить» ───────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn install_sources_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let their: Vec<(String, bool, String)> = g["tree_grid"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "button" && e["text"].as_str().unwrap_or("").contains('\n'))
        .map(|e| {
            (
                e["text"].as_str().unwrap().to_string(),
                e["enabled"].as_bool().unwrap(),
                e["tooltip"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mine: Vec<(String, bool, String)> = qe::install_sources()
        .iter()
        .map(|s| (s.text(), s.enabled, s.tooltip.clone()))
        .collect();
    assert_eq!(mine, their, "блок «Установить» разошёлся с оригиналом");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn page_chrome_matches_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let tree = g["tree_grid"].as_array().unwrap();
    let texts = labels(&g["tree_grid"]);
    assert!(texts.contains(&qe::header_title()));
    assert!(texts.contains(&qe::install_section_header()));
    assert!(texts.contains(&qe::roadmap_note()));

    let placeholders: Vec<String> = tree
        .iter()
        .filter(|e| e["kind"] == "edit")
        .map(|e| e["placeholder"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(placeholders, vec![qe::search_placeholder()]);

    let tool_buttons: Vec<(String, String)> = tree
        .iter()
        .filter(|e| e["kind"] == "tool")
        .map(|e| {
            (
                e["text"].as_str().unwrap().to_string(),
                e["tooltip"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        tool_buttons[0],
        (
            qe::VIEW_GRID_TEXT.to_string(),
            qe::view_grid_tooltip()
        )
    );
    assert_eq!(
        tool_buttons[1],
        (
            qe::VIEW_LIST_TEXT.to_string(),
            qe::view_list_tooltip()
        )
    );

    let plain_buttons: Vec<String> = tree
        .iter()
        .filter(|e| e["kind"] == "button" && !e["text"].as_str().unwrap_or("").contains('\n'))
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect();
    assert!(
        plain_buttons.contains(&qe::install_button_text()),
        "кнопка установки в шапке разошлась: {plain_buttons:?}"
    );
}

// ── подтверждения ───────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn confirmations_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    assert_eq!(qe::confirm_title(), "Удалить?");
    assert_eq!(qe::confirm_uninstall("Утилита X"), "Удалить «Утилита X»?");
    assert_eq!(
        qe::confirm_uninstall_detailed("Утилита X", "/дом/ext"),
        "Удалить «Утилита X»?\n\nФайлы из /дом/ext будут безвозвратно удалены."
    );
}

// ── починки оригинала (правило #16) ─────────────────────────────────────

#[test]
fn fix_1_markup_in_text_survives() {
    let g = golden();
    let fx = Fixture::new("escape", &g);
    let views = fx.views();
    let v = by_id(&views, "my-plugin");

    // Оригинал терял слово: QTextBrowser съедал `<изменений>` как тег —
    // это видно прямо в снимке.
    let their_plain = g["details"]["theme-dark"]["body_plain"].as_str().unwrap();
    assert!(
        !their_plain.contains("изменений"),
        "снимок оригинала обязан показывать потерю слова — иначе починка \
         описана неверно"
    );

    let d = qe::detail(v, None, "");
    assert!(
        d.heading_html.contains("&lt;разметкой&gt;"),
        "имя с угловыми скобками обязано экранироваться: {}",
        d.heading_html
    );
    let row = qe::list_row(v);
    assert!(row.title_html.contains("&lt;разметкой&gt;"));
    assert!(row.title_html.contains("&amp;"));
}

#[test]
fn fix_2_card_order_is_stable() {
    let g = golden();
    let fx = Fixture::new("order", &g);
    let first: Vec<String> = fx.views().iter().map(|v| v.ext_id.clone()).collect();
    let second: Vec<String> = fx.views().iter().map(|v| v.ext_id.clone()).collect();
    assert_eq!(first, second, "порядок обязан быть повторяемым");
    let mut sorted = first.clone();
    sorted.sort();
    // Идентификаторы совпадают с именами каталогов, поэтому отсортированный
    // обход каталога даёт отсортированный порядок карточек.
    assert_eq!(first, sorted, "карточки идут в порядке имён каталогов");
}

#[test]
fn fix_3_empty_state_is_one_decision() {
    // Обработчик фильтра в оригинале прятал подсказку «Расширений ещё нет».
    // Теперь ответ один и не зависит от того, кто спрашивает.
    assert_eq!(qe::empty_text(0, 0), Some(qe::empty_no_extensions()));
    assert_eq!(qe::empty_text(0, 3), Some(qe::empty_no_match()));
    assert_eq!(qe::empty_text(2, 3), None);
}

// ── сторож разметки: у страницы не должно быть своих цветов ─────────────

#[test]
fn page_colors_come_from_core() {
    let qml = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/qml/ExtensionsPage.qml"
    );
    let Ok(text) = fs::read_to_string(Path::new(qml)) else {
        // Разметка живёт в соседнем крейте; если его нет — сторожить нечего.
        return;
    };
    for (num, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        assert!(
            !code.contains("Qt.rgba("),
            "строка {}: цвет собран в разметке — он обязан приезжать из ядра",
            num + 1
        );
        // Шестнадцатеричный цвет в кавычках — тоже своё решение.
        assert!(
            !code.contains("\"#"),
            "строка {}: цвет прибит в разметке — палитра приходит из ui_base",
            num + 1
        );
    }
}

// ── перевод: английский без русских букв ────────────────────────────────

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Расширение с английскими данными: всё русское в его подписях — наше.
fn sample(ext_type: &str, enabled: bool) -> qe::ExtView {
    qe::ExtView {
        ext_id: "night".into(),
        name: "Night".into(),
        version: "1.0".into(),
        author: "Jane".into(),
        author_known: true,
        ext_type: ext_type.into(),
        type_label: rexon_core::constants::type_label(ext_type).unwrap_or_default(),
        type_icon: "🎨".into(),
        tags: vec!["dark".into()],
        de_support: vec!["kde".into()],
        enabled,
        dir: "/home/u/.config/rexon/extensions/night".into(),
        format_version: "3".into(),
        rexon_min_version: "5.0".into(),
        homepage: "https://example.org".into(),
        license: "MIT".into(),
        ..Default::default()
    }
}

/// Английский интерфейс вкладки: ни одной русской буквы в подписях,
/// статусах, карточках, разборе и справке — забытая строка видна здесь.
#[test]
fn english_extensions_tab_has_no_russian() {
    let _lang = i18n_pin::en();
    let mut all = vec![
        qe::window_title(),
        qe::header_title(),
        qe::search_placeholder(),
        qe::install_button_text(),
        qe::view_grid_tooltip(),
        qe::view_list_tooltip(),
        qe::filter_all_label(),
        qe::install_section_header(),
        qe::roadmap_note(),
        qe::empty_no_extensions(),
        qe::empty_no_match(),
        qe::uninstall_text(),
        qe::uninstall_tooltip(),
        qe::state_enabled_text(),
        qe::state_disabled_text(),
        qe::file_dialog_title(),
        qe::file_dialog_filter(),
        qe::status_install_busy(),
        qe::status_uninstall_busy(),
        qe::status_installing("a.rxn"),
        qe::status_installed("Night"),
        qe::status_install_error("x"),
        qe::status_uninstalling("Night"),
        qe::status_uninstalled("night"),
        qe::status_uninstall_error("x"),
        qe::status_toggled(true, "night"),
        qe::status_toggled(false, "night"),
        qe::status_toggle_error("x"),
        qe::confirm_title(),
        qe::confirm_uninstall("Night"),
        qe::confirm_uninstall_detailed("Night", "/d"),
        qe::detail_fallback_title(),
        qe::detail_open_folder_text(),
        qe::detail_uninstall_text(),
        qe::detail_close_text(),
        qe::detail_no_description(),
        qe::create_title(),
        qe::create_header(),
        qe::create_close_text(),
        qe::create_body_html(),
        qe::my_header(1, 3).unwrap(),
    ];
    for s in qe::install_sources() {
        all.extend([s.text(), s.tooltip]);
    }
    let types: Vec<String> = ["plugin", "theme-gtk", "wallpaper-pack"].map(String::from).to_vec();
    for item in qe::filter_items(&types) {
        all.push(item.label);
    }
    let views: Vec<qe::ExtView> = vec![sample("plugin", true), sample("theme-gtk", false)];
    all.push(qe::stats_line(&qe::stats(&views)));
    for v in &views {
        let c = qe::card(v);
        all.extend([c.type_line, c.author_line, c.state_text, c.toggle_text]);
        let r = qe::list_row(v);
        all.extend([r.subtitle, r.toggle_tooltip]);
        let d = qe::detail(v, None, "/home/u");
        all.extend([d.window_title, d.meta_line, d.body_html, d.tech_html, d.path_html]);
    }
    let refs: Vec<&qe::ExtView> = views.iter().collect();
    for g in qe::list_groups(&refs) {
        all.push(g.header_html);
    }
    assert!(all.len() > 60, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// Английские фразы читаются как фразы; «автор неизвестен» решает признак.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(qe::status_installing("a.rxn"), "⏳ Installing “a.rxn”…");
    assert_eq!(qe::confirm_uninstall("Night"), "Remove “Night”?");
    assert_eq!(qe::status_toggled(false, "night"), "⏸️ Disabled: night");
    assert_eq!(qe::my_header(1, 3).unwrap(), "My Extensions (1/3)");
    let d = qe::detail(&sample("plugin", true), None, "/home/u");
    // Версия формата пакета и его служебный номер человеку не показываются.
    assert!(!d.tech_html.contains("Format") && !d.tech_html.contains("ID:"), "{}", d.tech_html);
    assert!(d.tech_html.contains("<b>Minimum Rexon:</b> 5.0"), "{}", d.tech_html);
    assert!(d.tech_html.contains("<b>Website:</b> <a href='https://example.org'>"), "{}", d.tech_html);
    let mut v = sample("plugin", true);
    v.author_known = false;
    assert!(!v.has_author(), "автора нет в манифесте — строки автора нет");
    assert!(qe::card(&v).author_line.is_empty());
}

/// По-русски подписи без эталона Python — как были (сняты с HEAD 29.09).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    assert_eq!(qe::view_grid_tooltip(), "Сетка");
    assert_eq!(qe::view_list_tooltip(), "Список");
    assert_eq!(qe::uninstall_text(), "🗑️ Удалить");
    assert_eq!(qe::uninstall_tooltip(), "Удалить");
    assert_eq!(qe::toggle_on_text(), "▶️ Включить");
    assert_eq!(qe::toggle_off_text(), "⏸️ Выключить");
    assert_eq!(qe::state_enabled_text(), "✅ Активно");
    assert_eq!(qe::state_disabled_text(), "⏸️ Выключено");
    assert_eq!(qe::file_dialog_title(), "Выберите .rxn файл");
    assert_eq!(
        qe::file_dialog_filter(),
        "Rexon-расширения (*.rxn);;Windows темы (*.themepack *.deskthemepack *.theme *.msstyles);;Все файлы (*)"
    );
    assert_eq!(qe::status_toggle_error("x"), "⚠️ Ошибка переключения: x");
    assert_eq!(qe::detail_fallback_title(), "Расширение");
    assert_eq!(qe::detail_open_folder_text(), "📁 Открыть папку");
    assert_eq!(qe::detail_close_text(), "Закрыть");
    assert_eq!(qe::detail_no_description(), "<i>Нет описания</i>");
    let d = qe::detail(&sample("plugin", true), None, "/home/u");
    assert!(!d.tech_html.contains("Формат") && !d.tech_html.contains("ID:"), "{}", d.tech_html);
    assert!(d.tech_html.contains("<b>Rexon мин.:</b> 5.0"), "{}", d.tech_html);
    assert!(d.tech_html.contains("<b>Сайт:</b> <a href='https://example.org'>https://example.org</a>"));
    assert!(d.tech_html.contains("<b>Лицензия:</b> MIT"));
    // Справка — слово в слово прежняя (без ведущего перевода строки: в
    // HTML он не виден, а xgettext срезает его у ключа каталога).
    let before = include_str!("golden/qt_extensions_create_body_ru.html");
    assert_eq!(qe::create_body_html(), before);
}
