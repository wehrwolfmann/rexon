//! Golden-сверка порта `qt_compare.rs` с оригиналом —
//! `rexon/qt_compare/window.py` в версии до переноса (коммит `d305e9d`).
//!
//! Эталон снят прогоном ЖИВОГО модуля под offscreen-Qt
//! (`golden/qt_compare_cases.py` → `golden/qt_compare_cases.json`):
//!   * таблица значков столов и её запасное значение;
//!   * стиль карточек на тёмной и светлой темах;
//!   * полоса памяти на 22 значениях, включая ровно пороговые (419/420,
//!     779/780) и вылет за шкалу (1201, 5000);
//!   * `_stars` и `_value_style` на 27 значениях ТТХ;
//!   * ПОЛНОЕ дерево подписей собранного окна (308 узлов): заголовки,
//!     легенда, шапки и тела всех карточек, строки рекомендаций — потеря
//!     любой строки красит сверку;
//!   * таблица «X против Y» для семи пар и `_bar5`.
//!
//! Санкционированные расхождения (починки оригинала, правило #16) в голдене
//! НЕ отражены — там записано поведение Python как есть; каждая проверяется
//! отдельным тестом внизу файла:
//!   1. «X11 (Wayland WIP)» больше не зелёное;
//!   2. подсветка шапки и подложка полосы памяти зависят от темы;
//!   3. в диалоге сравнения обе кнопки установки выделены одинаково;
//!   4. оценки в таблице пары — значками (⭐/🎮 и ☆), как в легенде и
//!      карточках, а не полоской «█░» (`sanctioned_divergences`).
//!
//! ГЕЙТ «голден обязан ловить регрессию» проверен мутацией: семь точечных
//! поломок реализации (порог зелёного сдвинут на 0.36; `stars` не обрезает
//! пятёркой; в шапке карточки местами тулкит и композитор; в таблице пары
//! переставлены «Панель» и «Файлы»; `bar5` считает по девяти клеткам; значок
//! niri заменён запасным; в строке ТТХ потеряна «⚡ CPU нагрузка») — каждая
//! красит тесты.

use std::collections::BTreeMap;
use std::fs;

use rexon_core::i18n::tr;
use rexon_core::qt_compare as qc;
use rexon_core::system::{CpuLoad, DE_META};
use serde_json::Value;

mod i18n_pin;

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_compare_cases.json"
    );
    let text = fs::read_to_string(path).expect("нет снятого эталона qt_compare_cases.json");
    serde_json::from_str(&text).expect("эталон не разбирается как JSON")
}

fn installed_from_golden(g: &Value) -> Vec<String> {
    g["de_installed"]
        .as_object()
        .expect("de_installed — объект")
        .iter()
        .filter(|(_, v)| v.as_bool() == Some(true))
        .map(|(k, _)| k.clone())
        .collect()
}

// ── значки ──────────────────────────────────────────────────────────────

#[test]
fn de_icons_match_python() {
    let g = golden();
    let py: BTreeMap<String, String> =
        serde_json::from_value(g["de_icons"].clone()).expect("de_icons");
    let ours: BTreeMap<String, String> = qc::DE_ICONS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(ours, py, "таблица значков рабочих столов разошлась");

    let probe: BTreeMap<String, String> =
        serde_json::from_value(g["de_icon_probe"].clone()).expect("de_icon_probe");
    for (key, want) in probe {
        assert_eq!(qc::de_icon(&key), want, "значок «{key}»");
    }
}

/// Сторож: у каждого стола из базы есть свой значок.
///
/// Без него добавленный в ядро стол выехал бы на витрину с запасным
/// «🖥️» — ровно тот случай неполного добавления, который запрещает
/// правило #12.
#[test]
fn every_de_has_icon() {
    for m in DE_META {
        assert_ne!(
            qc::de_icon(m.id),
            qc::DE_ICON_FALLBACK,
            "у стола «{}» нет значка в qt_compare::DE_ICONS",
            m.id
        );
    }
}

/// Сторож: все идентификаторы в быстрых парах и рекомендациях существуют.
#[test]
fn quick_pairs_and_recommendations_point_at_known_de() {
    let known: Vec<&str> = DE_META.iter().map(|m| m.id).collect();
    for (label, a, b) in qc::QUICK_PAIRS {
        assert!(known.contains(a), "пара «{label}»: нет стола «{a}»");
        assert!(known.contains(b), "пара «{label}»: нет стола «{b}»");
    }
    for (label, _, de_id) in qc::RECOMMENDATIONS {
        assert!(
            known.contains(de_id),
            "рекомендация «{label}»: нет стола «{de_id}»"
        );
    }
}

// ── стиль карточек ──────────────────────────────────────────────────────

#[test]
fn card_qss_matches_python() {
    let g = golden();
    for (key, dark) in [("card_qss_dark", true), ("card_qss_light", false)] {
        let py: BTreeMap<String, String> =
            serde_json::from_value(g[key].clone()).expect("card_qss");
        for (name, want) in py {
            assert_eq!(qc::card_qss(&name, dark), want, "{key} / {name}");
        }
    }
}

// ── полоса памяти ───────────────────────────────────────────────────────

#[test]
fn ram_bar_matches_python() {
    let g = golden();
    let py = g["ram_bar"].as_object().expect("ram_bar — объект");
    for (mb_s, want) in py {
        let mb: u32 = mb_s.parse().expect("ключ полосы — число");
        // Эталон снят на светлой подложке (у Python она была единственной).
        let bar = qc::ram_bar(mb, false);
        assert_eq!(bar.max as i64, want["max"].as_i64().unwrap(), "max {mb}");
        assert_eq!(
            bar.value as i64,
            want["value"].as_i64().unwrap(),
            "value {mb}"
        );
        assert_eq!(
            bar.height as i64,
            want["height"].as_i64().unwrap(),
            "height {mb}"
        );
        assert_eq!(
            bar.text_visible,
            want["text_visible"].as_bool().unwrap(),
            "text_visible {mb}"
        );
        assert_eq!(bar.qss, want["qss"].as_str().unwrap(), "qss {mb}");
    }
}

// ── звёзды, цвет значения, полоска из десяти клеток ─────────────────────

#[test]
fn stars_match_python() {
    let g = golden();
    let py = g["stars"].as_object().expect("stars");
    for (n_s, want) in py {
        let n: u8 = n_s.parse().unwrap();
        let pair = want.as_array().expect("две строки");
        assert_eq!(qc::stars(n, "⭐"), pair[0].as_str().unwrap(), "⭐ {n}");
        assert_eq!(qc::stars(n, "🎮"), pair[1].as_str().unwrap(), "🎮 {n}");
    }
}

#[test]
fn value_style_matches_python_except_wayland_wip() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let py: BTreeMap<String, String> =
        serde_json::from_value(g["value_style"].clone()).expect("value_style");
    for (value, want) in py {
        if value.contains("WIP") {
            continue; // санкционированное расхождение, проверяется ниже
        }
        // Нагрузка — подпись признака: цвет решает вариант, не текст.
        let load = [CpuLoad::Low, CpuLoad::Medium, CpuLoad::High]
            .into_iter()
            .find(|l| l.label() == value);
        let ours = match load {
            Some(l) => qc::color_qss(qc::cpu_load_color(l)),
            None => qc::spec_value_qss(&value),
        };
        assert_eq!(ours, want, "цвет значения «{value}»");
    }
}

/// Полоска оригинала (`_bar5`) — только для сверки с эталоном: в таблице
/// пары её сменили значки оценки (санкция 4).
fn legacy_bar5(n: u8) -> String {
    let filled = (n.min(5) as usize) * 2;
    "█".repeat(filled) + &"░".repeat(10 - filled)
}

#[test]
fn legacy_bar5_matches_python() {
    let g = golden();
    let py = g["bar5"].as_object().expect("bar5");
    for (n_s, want) in py {
        let n: u8 = n_s.parse().unwrap();
        assert_eq!(legacy_bar5(n), want.as_str().unwrap(), "полоска {n}");
    }
}

// ── быстрые пары и рекомендации ─────────────────────────────────────────

#[test]
fn quick_pairs_match_python() {
    let g = golden();
    let py: Vec<Vec<String>> =
        serde_json::from_value(g["quick_pairs"].clone()).expect("quick_pairs");
    let ours: Vec<Vec<String>> = qc::QUICK_PAIRS
        .iter()
        .map(|(l, a, b)| vec![l.to_string(), a.to_string(), b.to_string()])
        .collect();
    assert_eq!(ours, py, "быстрые пары разошлись");
}

#[test]
fn recommendations_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let py: Vec<Vec<String>> =
        serde_json::from_value(g["recommendations"].clone()).expect("recommendations");
    let ours: Vec<Vec<String>> = qc::RECOMMENDATIONS
        .iter()
        .map(|(l, s, d)| vec![tr(l), tr(s), d.to_string()])
        .collect();
    assert_eq!(ours, py, "рекомендации разошлись");
}

// ── карточки ────────────────────────────────────────────────────────────

#[test]
fn card_headers_and_bodies_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let installed = installed_from_golden(&g);
    let cards = g["cards"].as_object().expect("cards");
    for (de_id, want) in cards {
        let is_installed = installed.iter().any(|i| i == de_id);
        assert_eq!(
            qc::card_header(de_id, false, is_installed).expect("шапка"),
            want["collapsed"].as_str().unwrap(),
            "шапка свёрнутой карточки {de_id}"
        );
        assert_eq!(
            qc::card_header(de_id, true, is_installed).expect("шапка"),
            want["expanded"].as_str().unwrap(),
            "шапка раскрытой карточки {de_id}"
        );

        let body = qc::card_body(de_id, is_installed, false).expect("тело");
        // Порядок подписей тела ровно тот, что видел покупатель: память,
        // две оценки, семь пар «подпись / значение», описание.
        let mut ours = vec![body.ram_label.clone()];
        ours.extend(body.stars_labels.clone());
        for s in &body.specs {
            ours.push(s.label.clone());
            ours.push(s.value_html.clone());
        }
        ours.push(body.description.clone());
        let py: Vec<String> =
            serde_json::from_value(want["body_labels"].clone()).expect("body_labels");
        let py: Vec<String> = py.iter().map(|t| sanctioned_divergences(t)).collect();
        assert_eq!(ours, py, "подписи тела карточки {de_id}");

        let py_btns: Vec<String> =
            serde_json::from_value(want["body_buttons"].clone()).expect("body_buttons");
        assert_eq!(
            vec![body.action.text.clone()],
            py_btns,
            "кнопка карточки {de_id}"
        );
    }
}

/// Санкционированные расхождения с оригиналом: слово «DE» и значки оценок в таблице пары.
///
/// Оригинал говорил «📊 Сравнение DE» и «Новый Wayland-native DE от System76»
/// (описание COSMIC приходит на карточку, в дерево окна и в таблицу пары); ядро
/// говорит «рабочих столов» / «рабочий стол» — сторож жаргона
/// `visible_texts_say_desktop_not_de` в qt_desktops_parity. Эталон заморожен и
/// переснимается из blob'а оригинала (`rexon-golden-update qt_compare`),
/// поэтому правится не он, а ожидание, и ровно эти две фразы.
fn sanctioned_divergences(original: &str) -> String {
    let text = original
        .replace("📊 Сравнение DE", "📊 Сравнение рабочих столов")
        .replace("Wayland-native DE от System76", "Wayland-native рабочий стол от System76");
    // Санкция 4: оценки в таблице пары — значками, как в легенде и
    // карточках; в строке «Гейминг» — геймпады, в остальных — звёзды.
    text.split_inclusive("</tr>")
        .map(|row| {
            let emoji = if row.contains("<td><b>Гейминг</b></td>") { "🎮" } else { "⭐" };
            (0..=5u8).fold(row.to_string(), |r, n| r.replace(&legacy_bar5(n), &qc::stars(n, emoji)))
        })
        .collect()
}

#[test]
fn sanctioned_divergences_are_exactly_the_de_word() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    assert_eq!(qc::header_title(), sanctioned_divergences("📊 Сравнение DE"));
    let g = golden();
    let text = g.to_string();
    // Эталон по-прежнему оригинальный: санкция на месте, а не устарела.
    assert_eq!(text.matches("📊 Сравнение DE").count(), 1);
    assert_eq!(text.matches("Wayland-native DE от System76").count(), 3);
    assert!(text.contains("<td><b>Гейминг</b></td><td>█"), "полоски оригинала в эталоне пары");
    assert!(!sanctioned_divergences(&text)
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "DE"));
}

// ── дерево подписей всего окна ──────────────────────────────────────────

/// Самая жёсткая сверка: все подписи окна в порядке обхода дерева.
///
/// Собираем ту же последовательность из ядра и сравниваем с записанной с
/// живого окна. Ловит и потерянную строку, и переставленные блоки.
#[test]
fn window_texts_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let installed = installed_from_golden(&g);
    let order: Vec<String> = serde_json::from_value(g["de_order"].clone()).expect("de_order");

    let mut ours: Vec<String> = vec![qc::header_title(), qc::pairs_label()];
    for (label, _, _) in qc::QUICK_PAIRS {
        ours.push(label.to_string());
    }
    ours.push(qc::legend_title());
    ours.push(qc::legend_body());
    ours.push(qc::table_header());
    for de_id in &order {
        let is_installed = installed.iter().any(|i| i == de_id);
        ours.push(qc::card_header(de_id, false, is_installed).expect("шапка"));
        let body = qc::card_body(de_id, is_installed, false).expect("тело");
        ours.push(body.ram_label.clone());
        ours.extend(body.stars_labels.clone());
        for s in &body.specs {
            ours.push(s.label.clone());
            ours.push(s.value_html.clone());
        }
        ours.push(body.description.clone());
        ours.push(body.action.text.clone());
    }
    ours.push(qc::rec_header());
    for row in qc::rec_rows(&installed) {
        ours.push(row.label_html.clone());
        ours.push(row.sub.clone());
        if !row.text.is_empty() {
            ours.push(row.text.clone());
        }
    }

    let tree = g["window_tree"].as_array().expect("window_tree");
    let py: Vec<String> = tree
        .iter()
        .filter_map(|e| e.get("text").and_then(|t| t.as_str()).map(sanctioned_divergences))
        .collect();
    assert_eq!(ours, py, "дерево подписей окна разошлось");
}

/// Полосы памяти в собранном окне — по одной на карточку, в том же порядке.
#[test]
fn window_ram_bars_match_python() {
    let g = golden();
    let order: Vec<String> = serde_json::from_value(g["de_order"].clone()).expect("de_order");
    let ours: Vec<(i64, i64)> = order
        .iter()
        .map(|id| {
            let b = qc::card_body(id, false, false).expect("тело");
            (b.ram_bar.max as i64, b.ram_bar.value as i64)
        })
        .collect();
    let py: Vec<(i64, i64)> = g["window_tree"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e.get("bar").and_then(|b| b.as_array()))
        .map(|a| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap()))
        .collect();
    assert_eq!(ours, py, "полосы памяти в окне разошлись");
}

// ── диалог «X против Y» ─────────────────────────────────────────────────

#[test]
fn pair_html_and_title_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let g = golden();
    let html = g["pair_html"].as_object().expect("pair_html");
    for (key, want) in html {
        let (a, b) = key.split_once('|').expect("ключ пары");
        let view = qc::pair_view(a, b, true, true).expect("пара");
        assert_eq!(view.html, sanctioned_divergences(want.as_str().unwrap()), "таблица пары {key}");
    }
    let titles = g["pair_title"].as_object().expect("pair_title");
    for (key, want) in titles {
        let (a, b) = key.split_once('|').expect("ключ пары");
        let view = qc::pair_view(a, b, true, true).expect("пара");
        assert_eq!(
            view.window_title,
            want.as_str().unwrap(),
            "заголовок пары {key}"
        );
    }
}

#[test]
fn pair_view_unknown_de_is_none() {
    assert!(qc::pair_view("kde", "нет-такого", false, false).is_none());
    assert!(qc::pair_view("нет-такого", "kde", false, false).is_none());
}

// ════════════════════════════════════════════════════════════════════════
//  Санкционированные расхождения с оригиналом (правило #16)
// ════════════════════════════════════════════════════════════════════════

/// Починка 1: «X11 (Wayland WIP)» — не готовый Wayland, зелёным не красим.
#[test]
fn wayland_wip_is_not_green() {
    assert_eq!(qc::spec_value_qss("X11 (Wayland WIP)"), "");
    // а настоящий Wayland по-прежнему зелёный
    assert_eq!(
        qc::spec_value_qss("Wayland"),
        format!("color: {};", qc::GOOD_COLOR)
    );
    assert_eq!(
        qc::spec_value_qss("Wayland + X11"),
        format!("color: {};", qc::GOOD_COLOR)
    );
    // и починка видна на витрине: у Budgie и LXQt сессия перестала быть
    // зелёной, а у KDE и GNOME осталась
    for m in DE_META {
        let green = !qc::spec_value_qss(m.session_type).is_empty();
        let expect_green =
            m.session_type.contains("Wayland") && !m.session_type.contains("WIP");
        assert_eq!(green, expect_green, "цвет сессии стола {}", m.id);
    }
}

/// Починка 2: подсветка шапки и подложка полосы зависят от темы.
#[test]
fn hover_and_track_follow_theme() {
    let light = qc::card_header_qss(false);
    let dark = qc::card_header_qss(true);
    assert!(light.contains("rgba(0,0,0,0.04)"), "светлая тема как раньше");
    assert!(dark.contains("rgba(255,255,255,0.06)"), "тёмная — светлее");
    assert_ne!(light, dark);

    assert!(qc::ram_bar(300, false).qss.contains("rgba(0,0,0,0.1)"));
    assert!(qc::ram_bar(300, true).qss.contains("rgba(255,255,255,0.10)"));
    // цвет заполнения от темы НЕ зависит — он несёт смысл
    assert_eq!(qc::ram_bar(300, false).color, qc::ram_bar(300, true).color);
}

/// Починка 3: обе кнопки установки в диалоге выделены одинаково.
#[test]
fn both_pair_install_buttons_are_primary() {
    let view = qc::pair_view("xfce", "kde", false, false).expect("пара");
    assert_eq!(view.buttons.len(), 2);
    for b in &view.buttons {
        assert_eq!(b.qss, qc::INSTALL_BUTTON_QSS, "кнопка {}", b.de_id);
    }
    // установленный стол кнопки не получает
    let view = qc::pair_view("xfce", "kde", true, false).expect("пара");
    assert_eq!(view.buttons.len(), 1);
    assert_eq!(view.buttons[0].de_id, "kde");
}

// ── строки рекомендаций ─────────────────────────────────────────────────

#[test]
fn rec_rows_mark_installed_and_offer_install() {
    let rows = qc::rec_rows(&["kde".to_string()]);
    assert_eq!(rows.len(), qc::RECOMMENDATIONS.len());
    for row in &rows {
        let want = if row.de_id == "kde" { "badge" } else { "install" };
        assert_eq!(row.mark, want, "отметка строки «{}»", row.label);
    }
    let kde = rows.iter().find(|r| r.de_id == "kde").unwrap();
    assert_eq!(kde.text, qc::REC_BADGE_TEXT);
    assert_eq!(kde.qss, qc::REC_BADGE_QSS);
}

/// Цвета вкладки описаны дважды: строкой QSS для виджетов и голым цветом для
/// QML. Разъехавшись, они дали бы двум вкладкам разный вид на одних данных —
/// и заметно это стало бы не на сборке, а глазами покупателя.
#[test]
fn widget_qss_and_qml_colors_agree() {
    assert_eq!(qc::REC_BADGE_QSS, format!("color: {};", qc::REC_BADGE_COLOR));
    for m in rexon_core::system::DE_META {
        for value in [
            m.toolkit,
            m.session_type,
            m.compositor,
            m.panel_type,
            m.file_manager,
            m.terminal,
        ] {
            let want = match qc::spec_value_color(value) {
                "" => String::new(),
                color => format!("color: {color};"),
            };
            assert_eq!(qc::spec_value_qss(value), want, "цвет значения «{value}»");
        }
        // Строки карточки несут цвет и стиль одним решением.
        for row in qc::specs(m.id) {
            assert_eq!(row.qss, qc::color_qss(row.color), "{} / {}", m.id, row.label);
        }
    }

    // Канавка полосы памяти: в QSS она записана как `rgba(r,g,b,a)`, в QML —
    // как `#AARRGGBB`. Сторож переводит первую во вторую и сверяет.
    for dark in [false, true] {
        let qss = qc::ram_bar(300, dark).qss;
        let css = qss
            .split("background: ")
            .nth(1)
            .and_then(|t| t.split(';').next())
            .expect("в QSS полосы нет подложки");
        assert_eq!(
            rgba_css_to_argb_hex(css),
            qc::ram_track_color(dark),
            "канавка полосы памяти разъехалась (тёмная тема: {dark})"
        );
    }

    // Синяя кнопка установки: подложка и подложка под курсором в QSS и в
    // QML-константах — один и тот же цвет, а скругление — то же число.
    let mut backgrounds = qc::INSTALL_BUTTON_QSS
        .split("background-color: ")
        .skip(1)
        .map(|t| t.split(';').next().expect("подложка кнопки без «;»"));
    let plain = backgrounds.next().expect("в QSS кнопки нет подложки");
    let hover = backgrounds.next().expect("в QSS кнопки нет подложки под курсором");
    assert_eq!(rgba_css_to_argb_hex(plain), qc::INSTALL_BUTTON_COLOR);
    assert_eq!(rgba_css_to_argb_hex(hover), qc::INSTALL_BUTTON_HOVER_COLOR);
    assert!(
        qc::INSTALL_BUTTON_QSS.contains(&format!("border-radius: {}px", qc::INSTALL_BUTTON_RADIUS)),
        "скругление кнопки установки разъехалось"
    );
}

/// `rgba(r,g,b,a)` из QSS → `#AARRGGBB`, как этот же цвет пишут в QML.
fn rgba_css_to_argb_hex(css: &str) -> String {
    let inner = css
        .trim()
        .trim_start_matches("rgba(")
        .trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    assert_eq!(parts.len(), 4, "не rgba(): {css}");
    let r: u8 = parts[0].parse().expect("красный");
    let g: u8 = parts[1].parse().expect("зелёный");
    let b: u8 = parts[2].parse().expect("синий");
    let a = (parts[3].parse::<f64>().expect("прозрачность") * 255.0).round() as u8;
    format!("#{a:02x}{r:02x}{g:02x}{b:02x}")
}

/// Затемнение фона — одно решение на продукт. Гасят фон двое: обучение
/// (накладка на всё окно) и модальное окно вкладки на QML; разъехавшись,
/// они показали бы покупателю два разных «выключенных» фона в одном окне.
/// Сторож смотрит и на .qml: там цвет обязан приезжать из палитры, а не
/// быть набранным руками.
#[test]
fn modal_dim_is_one_decision() {
    assert_eq!(
        rexon_core::onboarding::DIM_COLOR,
        rexon_core::ui_base::MODAL_DIM_RGBA,
        "обучение и модальные окна гасят фон разным цветом"
    );
    assert_eq!(rexon_core::ui_base::modal_dim_argb_hex(), "#aa000000");

    let qml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/qml/ComparePage.qml"
    );
    let qml = fs::read_to_string(qml_path).expect("нет разметки ComparePage.qml");
    assert!(
        qml.contains("color: root.pal.modal_dim"),
        "затемнение в ComparePage.qml не берётся из палитры"
    );
    assert!(
        !qml.contains("Qt.rgba("),
        "в ComparePage.qml снова набран цвет руками"
    );
}

// ── тексты установки и переключения ─────────────────────────────────────

#[test]
fn install_and_switch_texts_are_intact() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let t = qc::install_texts("KDE Plasma");
    assert_eq!(t.confirm_title, "Установить?");
    assert!(t.confirm_html.contains("<b>KDE Plasma</b>"));
    assert!(t.confirm_html.contains("пароль администратора"));
    assert_eq!(t.operation_title, "Установка KDE Plasma");
    assert!(t.done_text.starts_with("<b>KDE Plasma</b> установлен."));

    let s = qc::switch_texts("XFCE");
    assert_eq!(s.confirm_title, "Переключиться?");
    assert!(s.confirm_text.contains("<b>XFCE</b>"));
    assert_eq!(s.operation_title, "Переключение на XFCE");
}

// ════════════════════════════════════════════════════════════════════════
//  Перевод: русский как было, английский — без единой русской буквы
// ════════════════════════════════════════════════════════════════════════

/// ⭐ Русский интерфейс вкладки не изменился: подписи, которых нет в эталоне
/// Python (заголовок окна, кнопки диалога пары, тексты переключения),
/// сверяются с прежними литералами слово в слово (сняты с HEAD 28.09);
/// остальное сторожат golden-сверки выше, идущие на `po/ru.po`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    assert_eq!(qc::window_title(), "Rexon — Сравнение рабочих столов");
    assert_eq!(qc::pair_close_text(), "Закрыть");
    let view = qc::pair_view("xfce", "kde", false, false).unwrap();
    assert_eq!(view.buttons[0].text, "📦 Установить XFCE");
    assert_eq!(view.window_title, "⚔️ XFCE ↔ KDE Plasma");
    assert_eq!(qc::card_action("kde", true).text, "🔄 Переключиться");
    assert_eq!(qc::card_action("kde", false).text, "📦 Установить");
    let s = qc::switch_texts("XFCE");
    assert_eq!(s.confirm_title, "Переключиться?");
    assert_eq!(
        s.confirm_text,
        "Переключиться на <b>XFCE</b>?\n\n\
Rexon сохранит копию настроек текущего рабочего стола и выйдет из сеанса."
    );
    assert_eq!(s.operation_title, "Переключение на XFCE");
    // Нагрузка — подпись признака, у каждого варианта своя.
    assert_eq!(CpuLoad::Low.label(), "Низкое");
    assert_eq!(CpuLoad::Medium.label(), "Среднее");
    assert_eq!(CpuLoad::High.label(), "Высокое");
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все тексты вкладки для всех столов — как их видит покупатель.
fn all_texts() -> Vec<String> {
    let mut all = vec![
        qc::window_title(),
        qc::header_title(),
        qc::pairs_label(),
        qc::legend_title(),
        qc::legend_body(),
        qc::table_header(),
        qc::rec_header(),
        qc::pair_close_text(),
    ];
    for m in DE_META {
        all.push(qc::card_header(m.id, true, true).unwrap());
        for installed in [false, true] {
            let body = qc::card_body(m.id, installed, true).unwrap();
            all.push(body.ram_label);
            all.extend(body.stars_labels);
            for s in body.specs {
                all.extend([s.label, s.value_html]);
            }
            all.push(body.description);
            all.push(body.action.text);
        }
        let s = qc::switch_texts(m.display_name);
        all.extend([s.confirm_title, s.confirm_text, s.operation_title]);
    }
    for row in qc::rec_rows(&["kde".to_string()]) {
        all.extend([row.label_html, row.sub, row.text]);
    }
    for (_, a, b) in qc::QUICK_PAIRS {
        let v = qc::pair_view(a, b, false, false).unwrap();
        all.extend([v.window_title, v.heading, v.html, v.close_text]);
        all.extend(v.buttons.into_iter().map(|b| b.text));
    }
    all
}

/// Английский интерфейс вкладки: ни одной русской буквы — забытая строка
/// видна здесь, а не покупателю.
///
/// Исключение названо: тексты УСТАНОВКИ стола (`install_texts`) берутся у
/// `qt_desktops` — это заход вкладки «Рабочие столы», сюда не входят.
#[test]
fn english_compare_has_no_russian() {
    let _lang = i18n_pin::en();
    let all = all_texts();
    assert!(all.len() > 200, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// Английские фразы читаются как фразы, а цвет нагрузки не потерялся с
/// переводом подписи.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(qc::header_title(), "📊 Desktop Comparison");
    assert_eq!(
        qc::card_header("kde", false, true).unwrap(),
        "▶  💎  KDE Plasma  ✅   ·   Memory 700 MB   ·   Processor Medium   ·   Qt6   ·   KWin"
    );
    let body = qc::card_body("xfce", false, true).unwrap();
    assert_eq!(body.ram_label, "<b>Memory:</b>  300 MB");
    assert_eq!(body.stars_labels[1], "<b>Gaming:</b>  🎮🎮🎮🎮🎮");
    let cpu = body.specs.last().unwrap();
    assert_eq!(cpu.label, "⚡ CPU load");
    assert_eq!(cpu.value, "Low");
    assert_eq!(cpu.color, qc::GOOD_COLOR, "низкая нагрузка по-английски тоже зелёная");
    let gnome = qc::specs("gnome");
    assert_eq!(gnome.last().unwrap().color, qc::BAD_COLOR);
    let lxqt = qc::specs("lxqt");
    assert_eq!(lxqt[2].value, "none (Openbox WM; labwc optional)");
    let s = qc::switch_texts("XFCE");
    assert_eq!(s.confirm_title, "Switch Desktops?");
    assert_eq!(
        s.confirm_text,
        "Switch to <b>XFCE</b>?\n\nRexon will back up the current desktop's settings and log you out."
    );
    let v = qc::pair_view("xfce", "kde", false, false).unwrap();
    assert_eq!(v.buttons[0].text, "📦 Install XFCE");
    assert!(v.html.contains("<th align='left'>Feature</th>"));
    assert!(v.html.contains("<td><b>CPU load</b></td><td>Low</td><td>Medium</td>"));
}

/// Цвет нагрузки одинаков на любом языке: решает признак, а не подпись.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn cpu_color_does_not_depend_on_language() {
    let colors = |_: ()| -> Vec<&'static str> {
        DE_META.iter().map(|m| qc::specs(m.id).last().unwrap().color).collect()
    };
    let en = {
        let _lang = i18n_pin::en();
        colors(())
    };
    let ru = {
        let _lang = i18n_pin::ru();
        colors(())
    };
    assert_eq!(en, ru);
    assert!(en.contains(&qc::GOOD_COLOR) && en.contains(&qc::BAD_COLOR));
}
