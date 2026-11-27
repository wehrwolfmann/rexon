//! Golden check of the `qt_compare.rs` port against the original —
//! `rexon/qt_compare/window.py` in its version before the move (commit `d305e9d`).
//!
//! The golden was captured by running the LIVE module under offscreen Qt
//! (`golden/qt_compare_cases.py` → `golden/qt_compare_cases.json`):
//!   * the table of desktop icons and its fallback value;
//!   * the card style on dark and light themes;
//!   * the memory bar on 22 values, including exactly the thresholds (419/420,
//!     779/780) and going off the scale (1201, 5000);
//!   * `_stars` and `_value_style` on 27 spec values;
//!   * the FULL label tree of the built window (308 nodes): titles,
//!     legend, headers and bodies of all cards, recommendation lines — losing
//!     any line turns the check red;
//!   * the "X versus Y" table for seven pairs and `_bar5`.
//!
//! Sanctioned deviations (fixes of the original, rule #16) are NOT reflected
//! in the golden — it records Python's behaviour as is; each one is checked by
//! a separate test at the bottom of the file:
//!   1. «X11 (Wayland WIP)» is no longer green;
//!   2. the header highlight and the memory bar backing depend on the theme;
//!   3. in the comparison dialog both install buttons are highlighted the same way;
//!   4. ratings in the pair table are icons (⭐/🎮 and ☆), as in the legend and
//!      the cards, not a «█░» bar (`sanctioned_divergences`);
//!   5. the words «Игры» and «Библиотека интерфейса» instead of «Гейминг» and «Тулкит»;
//!   6. session labels per the Arch packages as of 02.11.2026 (`SESSION_SANCTION`);
//!   7. the interface library, compositor and terminal of Budgie 10.10
//!      (`SPEC_SANCTION`), its file manager (`FM_SANCTION`).
//!
//! The GATE "the golden must catch a regression" was checked by mutation: seven point
//! breakages of the implementation (the green threshold shifted by 0.36; `stars` does not clamp
//! at five; the toolkit and the compositor swapped in the card header; «Panel» and «Files»
//! swapped in the pair table; `bar5` counting over nine cells; the niri icon
//! replaced by the fallback; «⚡ CPU load» lost from the spec line) — each one
//! turns the tests red.

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

// ── icons ───────────────────────────────────────────────────────────────

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

/// Guard: every desktop in the database has its own icon.
///
/// Without it a desktop added to the core would go to the storefront with the fallback
/// «🖥️» — exactly the case of an incomplete addition that rule #12
/// forbids.
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

/// Guard: all identifiers in the quick pairs and recommendations exist.
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

// ── card style ──────────────────────────────────────────────────────────

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

// ── memory bar ──────────────────────────────────────────────────────────

#[test]
fn ram_bar_matches_python() {
    let g = golden();
    let py = g["ram_bar"].as_object().expect("ram_bar — объект");
    for (mb_s, want) in py {
        let mb: u32 = mb_s.parse().expect("ключ полосы — число");
        // The golden was captured on a light backing (Python had only that one).
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

// ── stars, value colour, a bar of ten cells ─────────────────────────────

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
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let g = golden();
    let py: BTreeMap<String, String> =
        serde_json::from_value(g["value_style"].clone()).expect("value_style");
    for (value, want) in py {
        if value.contains("WIP") {
            continue; // a sanctioned deviation, checked below
        }
        // Load is a feature label: the variant decides the colour, not the text.
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

/// The original's bar (`_bar5`) — only for comparison with the golden: in the pair
/// table it was replaced by rating icons (sanction 4).
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

// ── quick pairs and recommendations ─────────────────────────────────────

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
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let g = golden();
    let py: Vec<Vec<String>> =
        serde_json::from_value(g["recommendations"].clone()).expect("recommendations");
    let ours: Vec<Vec<String>> = qc::RECOMMENDATIONS
        .iter()
        .map(|(l, s, d)| vec![tr(l), tr(s), d.to_string()])
        .collect();
    assert_eq!(ours, py, "рекомендации разошлись");
}

// ── cards ───────────────────────────────────────────────────────────────

#[test]
fn card_headers_and_bodies_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let g = golden();
    let installed = installed_from_golden(&g);
    let cards = g["cards"].as_object().expect("cards");
    for (de_id, want) in cards {
        let is_installed = installed.iter().any(|i| i == de_id);
        assert_eq!(
            qc::card_header(de_id, false, is_installed).expect("шапка"),
            sanction_specs(want["collapsed"].as_str().unwrap()),
            "шапка свёрнутой карточки {de_id}"
        );
        assert_eq!(
            qc::card_header(de_id, true, is_installed).expect("шапка"),
            sanction_specs(want["expanded"].as_str().unwrap()),
            "шапка раскрытой карточки {de_id}"
        );

        let body = qc::card_body(de_id, is_installed, false).expect("тело");
        // The order of the body labels is exactly what the buyer saw: memory,
        // two ratings, seven "label / value" pairs, the description.
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
        let py = sanction_sessions(py, &[de_id.as_str()]);
        let py = sanction_fms(py, &[de_id.as_str()]);
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

/// Sanction 6: session labels went stale together with the desktops themselves, not because of a port
/// bug. The facts as of 02.11.2026 (Arch packages): Plasma 6.8 (`plasma-workspace`
/// ships only `plasma.desktop`, `plasma-x11-session` was removed from the
/// repositories), GNOME 51 (`gnome-session` — only `gnome.desktop`), Budgie 10.10
/// (Wayland via labwc) — Wayland only. Cinnamon, Xfce and LXQt have
/// a session choice at install time (MATE since 16.11.2026 — X11 only, as in the golden), and the label is derived from the variant table
/// `system::SESSION_VARIANTS` (`DeMeta::session_text`). The label takes the
/// "experimental" mark from the Wayland maturity flag in the same place: as of
/// 03.11.2026 only LXQt's is mature (the project dropped the mark in LXQt 2.3.0).
/// Pantheon is not in the golden — its session is checked in `system_de_db.json`.
///
/// Desktop → (value in the original's golden, value now, in Russian).
const SESSION_SANCTION: &[(&str, &str, &str)] = &[
    ("cinnamon-mint", "X11", "X11 + Wayland (экспериментально)"),
    ("xfce", "X11", "X11 + Wayland (экспериментально)"),
    ("lxqt", "X11 (Wayland WIP)", "X11 + Wayland"),
    ("kde", "Wayland + X11", "Wayland"),
    ("gnome", "Wayland + X11", "Wayland"),
    ("budgie", "X11 (Wayland WIP)", "Wayland"),
];

/// The desktop's session value after sanction 6. The sanction is pinpoint: the golden
/// must contain exactly the recorded old value — otherwise it is stale.
fn session_now(de_id: &str, original: &str) -> String {
    match SESSION_SANCTION.iter().find(|(id, _, _)| *id == de_id) {
        Some((_, old, new)) => {
            assert_eq!(original, *old, "санкция сеанса стола {de_id} не совпала с эталоном");
            new.to_string()
        }
        None => original.to_string(),
    }
}

/// Sanction 6 in the label list: the value after the «🖥️ Session» line. Session
/// lines go one per card, in `order`.
fn sanction_sessions(labels: Vec<String>, order: &[&str]) -> Vec<String> {
    let mut des = order.iter();
    let mut pending: Option<&str> = None;
    let mut out = Vec::with_capacity(labels.len());
    for text in labels {
        if let Some(de_id) = pending.take() {
            let value = text
                .strip_prefix("<b>")
                .and_then(|t| t.strip_suffix("</b>"))
                .expect("значение сеанса — <b>…</b>");
            out.push(format!("<b>{}</b>", session_now(de_id, value)));
            continue;
        }
        if text == "🖥️ Сессия" {
            pending = Some(des.next().expect("строк сеанса больше, чем карточек"));
        }
        out.push(text);
    }
    assert!(des.next().is_none(), "у карточки потерялась строка сеанса");
    out
}

/// Sanction 6 in the "X versus Y" pair table: the «Session» row.
fn sanction_pair_session(html: &str, a: &str, b: &str) -> String {
    const HEAD: &str = "<tr><td><b>Сессия</b></td><td>";
    html.split_inclusive("</tr>")
        .map(|row| match row.strip_prefix(HEAD) {
            Some(rest) => {
                let (va, vb) = rest
                    .strip_suffix("</td></tr>")
                    .and_then(|r| r.split_once("</td><td>"))
                    .expect("строка сеанса пары");
                format!("{HEAD}{}</td><td>{}</td></tr>", session_now(a, va), session_now(b, vb))
            }
            None => row.to_string(),
        })
        .collect()
}

/// Sanction 6 is alive: every entry is needed (the desktop is in the golden) and matches
/// what the core shows; for "Wayland only" desktops it is exactly «Wayland».
#[test]
fn session_sanction_is_current() {
    let _lang = i18n_pin::ru();
    let g = golden();
    for (de_id, _, new) in SESSION_SANCTION {
        assert!(g["cards"].get(*de_id).is_some(), "стола {de_id} нет в эталоне — запись лишняя");
        let m = DE_META.iter().find(|m| m.id == *de_id).expect("стол в DE_META");
        assert_eq!(m.session_text(), *new, "сеанс {de_id}");
    }
    for de_id in ["kde", "gnome", "budgie", "pantheon", "cosmic", "deepin"] {
        let m = DE_META.iter().find(|m| m.id == de_id).expect("стол в DE_META");
        assert_eq!(m.session_text(), "Wayland", "сеанс {de_id}");
    }
}

/// Sanction 7: the Budgie specs went stale together with the desktop itself. The facts as of 02.11.2026
/// (Arch, `budgie-desktop` 10.10.2): windows are managed by labwc — the
/// `budgie-desktop.desktop` session starts `startbudgielabwc`, the package depends on
/// `labwc`; Budgie WM on Mutter left together with X11, Magpie never came out.
/// The desktop itself is on GTK3: `gtk3` is in the dependencies, GTK4 is not ("GTK3 → GTK4" is
/// a promise, not a state).
///
/// The terminal is `xfce4-terminal`, not «GNOME Terminal»: it is installed by the Budgie
/// package set (`budgie-desktop` itself does not pull in a terminal) and assigned as
/// the default program, as in Ubuntu Budgie on 10.10
/// (`ubuntu-budgie-desktop` recommends `xfce4-terminal`).
///
/// Desktop → (value in the original's golden, value now). Only this desktop has these values
/// in the golden, so the replacement is by text.
const SPEC_SANCTION: &[(&str, &str, &str)] = &[
    ("budgie", "GTK3 → GTK4", "GTK3"),
    ("budgie", "Budgie WM / Magpie", "labwc"),
    ("budgie", "GNOME Terminal", "xfce4-terminal"),
];

/// Sanction 7 in any golden text: the header, the card body, the pair table.
fn sanction_specs(original: &str) -> String {
    SPEC_SANCTION.iter().fold(original.to_string(), |t, (_, old, new)| t.replace(old, new))
}

/// Sanction 7 is alive: the old value is in the golden (in the header and body of its
/// desktop's card), the new one is in the desktop's meta.
#[test]
fn spec_sanction_is_current() {
    let g = golden();
    for (de_id, old, new) in SPEC_SANCTION {
        let card = g["cards"].get(*de_id).expect("стол санкции есть в эталоне").to_string();
        assert!(card.contains(old), "в эталоне карточки {de_id} нет «{old}» — санкция устарела");
        let m = DE_META.iter().find(|m| m.id == *de_id).expect("стол в DE_META");
        assert!(
            m.toolkit == *new || m.compositor == *new || m.terminal == *new,
            "у стола {de_id} в мете нет «{new}»"
        );
    }
}

/// Sanction 7, the Budgie file manager: Nemo, not Nautilus. The Budgie package set
/// and its default programs follow Ubuntu Budgie 26.04
/// (`ubuntu-budgie-desktop-minimal` recommends `nemo`, its `defaults.list`
/// opens folders with it). «Nautilus» in the golden is also GNOME's, so the replacement
/// is not by text but by desktop: the value after «📁 File manager» in
/// the card and the desktop's column in the «Files» row of the pair table.
///
/// Desktop → (value in the original's golden, value now).
const FM_SANCTION: &[(&str, &str, &str)] = &[("budgie", "Nautilus", "Nemo")];

/// The desktop's file manager after the sanction. Pinpoint: the golden must contain
/// exactly the recorded old value — otherwise it is stale.
fn fm_now(de_id: &str, original: &str) -> String {
    match FM_SANCTION.iter().find(|(id, _, _)| *id == de_id) {
        Some((_, old, new)) => {
            assert_eq!(original, *old, "санкция файлового менеджера {de_id} не совпала с эталоном");
            new.to_string()
        }
        None => original.to_string(),
    }
}

/// The sanction in the label list: the value after «📁 File manager», one
/// line per card in `order`.
fn sanction_fms(labels: Vec<String>, order: &[&str]) -> Vec<String> {
    let mut des = order.iter();
    let mut pending: Option<&str> = None;
    let mut out = Vec::with_capacity(labels.len());
    for text in labels {
        if let Some(de_id) = pending.take() {
            let value = text
                .strip_prefix("<b>")
                .and_then(|t| t.strip_suffix("</b>"))
                .expect("значение файлового менеджера — <b>…</b>");
            out.push(format!("<b>{}</b>", fm_now(de_id, value)));
            continue;
        }
        if text == "📁 Файловый менеджер" {
            pending = Some(des.next().expect("строк файлового менеджера больше, чем карточек"));
        }
        out.push(text);
    }
    assert!(des.next().is_none(), "у карточки потерялась строка файлового менеджера");
    out
}

/// The sanction in the "X versus Y" pair table: the «Files» row.
fn sanction_pair_fm(html: &str, a: &str, b: &str) -> String {
    const HEAD: &str = "<tr><td><b>Файлы</b></td><td>";
    html.split_inclusive("</tr>")
        .map(|row| match row.strip_prefix(HEAD) {
            Some(rest) => {
                let (va, vb) = rest
                    .strip_suffix("</td></tr>")
                    .and_then(|r| r.split_once("</td><td>"))
                    .expect("строка «Файлы» пары");
                format!("{HEAD}{}</td><td>{}</td></tr>", fm_now(a, va), fm_now(b, vb))
            }
            None => row.to_string(),
        })
        .collect()
}

/// The file manager sanction is alive: the desktop is in the golden, the new value is
/// in the desktop's meta.
#[test]
fn fm_sanction_is_current() {
    let g = golden();
    for (de_id, old, new) in FM_SANCTION {
        let card = g["cards"].get(*de_id).expect("стол санкции есть в эталоне").to_string();
        assert!(card.contains(old), "в эталоне карточки {de_id} нет «{old}» — санкция устарела");
        let m = DE_META.iter().find(|m| m.id == *de_id).expect("стол в DE_META");
        assert_eq!(m.file_manager, *new, "файловый менеджер {de_id} в мете");
    }
}

/// Sanctioned deviations from the original: the word "DE" and the rating icons in the pair table.
///
/// The original said «📊 Сравнение DE» and «Новый Wayland-native DE от System76»
/// (the COSMIC description goes to the card, the window tree and the pair table); the core
/// says «рабочих столов» / «рабочий стол» — the jargon guard
/// `visible_texts_say_desktop_not_de` in qt_desktops_parity. The golden is frozen and
/// recaptured from the original's blob (`rexon-golden-update qt_compare`),
/// so it is not the golden that is edited but the expectation, and exactly these two phrases.
fn sanctioned_divergences(original: &str) -> String {
    let text = sanction_specs(original)
        .replace("📊 Сравнение DE", "📊 Сравнение рабочих столов")
        .replace("Wayland-native DE от System76", "Wayland-native рабочий стол от System76");
    // Sanction 4: ratings in the pair table are icons, as in the legend and
    // the cards; gamepads in the «Gaming» row, stars in the others.
    text.split_inclusive("</tr>")
        .map(|row| {
            let emoji = if row.contains("<td><b>Гейминг</b></td>") { "🎮" } else { "⭐" };
            (0..=5u8).fold(row.to_string(), |r, n| r.replace(&legacy_bar5(n), &qc::stars(n, emoji)))
        })
        .collect::<String>()
        // Sanction 5: words as everywhere in Rexon — «Игры», not «Гейминг»;
        // «Библиотека интерфейса», not the jargon «Тулкит».
        .replace("Гейминг", "Игры")
        .replace("Тулкит", "Библиотека интерфейса")
}

#[test]
fn sanctioned_divergences_are_exactly_the_de_word() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    assert_eq!(qc::header_title(), sanctioned_divergences("📊 Сравнение DE"));
    let g = golden();
    let text = g.to_string();
    // The golden is still the original: the sanction is in place, not stale.
    assert_eq!(text.matches("📊 Сравнение DE").count(), 1);
    assert_eq!(text.matches("Wayland-native DE от System76").count(), 3);
    assert!(text.contains("<td><b>Гейминг</b></td><td>█"), "полоски оригинала в эталоне пары");
    assert!(!sanctioned_divergences(&text)
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "DE"));
}

// ── the label tree of the whole window ──────────────────────────────────

/// The strictest check: all window labels in tree traversal order.
///
/// We build the same sequence from the core and compare it with the one recorded from
/// the live window. It catches both a lost line and swapped blocks.
#[test]
fn window_texts_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
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
    let order: Vec<&str> = order.iter().map(String::as_str).collect();
    let py = sanction_sessions(py, &order);
    let py = sanction_fms(py, &order);
    assert_eq!(ours, py, "дерево подписей окна разошлось");
}

/// Memory bars in the built window — one per card, in the same order.
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

// ── the "X versus Y" dialog ─────────────────────────────────────────────

#[test]
fn pair_html_and_title_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let g = golden();
    let html = g["pair_html"].as_object().expect("pair_html");
    for (key, want) in html {
        let (a, b) = key.split_once('|').expect("ключ пары");
        let view = qc::pair_view(a, b, true, true).expect("пара");
        let want = sanction_pair_fm(want.as_str().unwrap(), a, b);
        let want = sanctioned_divergences(&sanction_pair_session(&want, a, b));
        assert_eq!(view.html, want, "таблица пары {key}");
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
//  Sanctioned deviations from the original (rule #16)
// ════════════════════════════════════════════════════════════════════════

/// Fix 1: «X11 (Wayland WIP)» is not a ready Wayland, we do not paint it green.
#[test]
fn wayland_wip_is_not_green() {
    assert_eq!(qc::spec_value_qss("X11 (Wayland WIP)"), "");
    // while real Wayland is still green
    assert_eq!(
        qc::spec_value_qss("Wayland"),
        format!("color: {};", qc::GOOD_COLOR)
    );
    assert_eq!(
        qc::spec_value_qss("Wayland + X11"),
        format!("color: {};", qc::GOOD_COLOR)
    );
    // and the fix is visible on the storefront: the session is green only for a ready Wayland;
    // desktops with experimental Wayland next to X11 (sanction 6) are not green
    for m in DE_META {
        let green = !qc::specs(m.id)[1].color.is_empty();
        let expect_green = m.session_type == "Wayland";
        assert_eq!(green, expect_green, "цвет сессии стола {}", m.id);
    }
}

/// Fix 2: the header highlight and the bar backing depend on the theme.
#[test]
fn hover_and_track_follow_theme() {
    let light = qc::card_header_qss(false);
    let dark = qc::card_header_qss(true);
    assert!(light.contains("rgba(0,0,0,0.04)"), "светлая тема как раньше");
    assert!(dark.contains("rgba(255,255,255,0.06)"), "тёмная — светлее");
    assert_ne!(light, dark);

    assert!(qc::ram_bar(300, false).qss.contains("rgba(0,0,0,0.1)"));
    assert!(qc::ram_bar(300, true).qss.contains("rgba(255,255,255,0.10)"));
    // the fill colour does NOT depend on the theme — it carries meaning
    assert_eq!(qc::ram_bar(300, false).color, qc::ram_bar(300, true).color);
}

/// Fix 3: both install buttons in the dialog are highlighted the same way.
#[test]
fn both_pair_install_buttons_are_primary() {
    let view = qc::pair_view("xfce", "kde", false, false).expect("пара");
    assert_eq!(view.buttons.len(), 2);
    for b in &view.buttons {
        assert_eq!(b.qss, qc::INSTALL_BUTTON_QSS, "кнопка {}", b.de_id);
    }
    // an installed desktop does not get a button
    let view = qc::pair_view("xfce", "kde", true, false).expect("пара");
    assert_eq!(view.buttons.len(), 1);
    assert_eq!(view.buttons[0].de_id, "kde");
}

// ── recommendation lines ────────────────────────────────────────────────

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

/// The tab's colours are described twice: as a QSS string for widgets and as a bare colour for
/// QML. If they diverged, two tabs would look different on the same data —
/// and it would be noticed not at build time but by the buyer's eyes.
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
        // Card lines carry colour and style as one decision.
        for row in qc::specs(m.id) {
            assert_eq!(row.qss, qc::color_qss(row.color), "{} / {}", m.id, row.label);
        }
    }

    // The memory bar groove: in QSS it is written as `rgba(r,g,b,a)`, in QML
    // as `#AARRGGBB`. The guard converts the first into the second and compares.
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

    // The blue install button: the backing and the hover backing in QSS and in
    // the QML constants are one and the same colour, and the rounding is the same number.
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

/// `rgba(r,g,b,a)` from QSS → `#AARRGGBB`, as the same colour is written in QML.
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

/// Background dimming is one decision for the whole product. Two parties dim the background: the tutorial
/// (an overlay over the whole window) and the QML tab's modal window; if they diverged,
/// they would show the buyer two different "disabled" backgrounds in one window.
/// The guard looks at the .qml too: there the colour must come from the palette rather than
/// be typed by hand.
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

// ── install and switch texts ────────────────────────────────────────────

#[test]
fn install_and_switch_texts_are_intact() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
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
//  Translation: Russian as before, English without a single Russian letter
// ════════════════════════════════════════════════════════════════════════

/// ⭐ The tab's Russian interface did not change: labels that are not in the Python
/// golden (the window title, the pair dialog buttons, the switch texts)
/// are compared with the former literals word for word (taken from HEAD on 28.09);
/// the rest is guarded by the golden checks above, which run on `po/ru.po`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
    // Load is a feature label, each variant has its own.
    assert_eq!(CpuLoad::Low.label(), "Низкое");
    assert_eq!(CpuLoad::Medium.label(), "Среднее");
    assert_eq!(CpuLoad::High.label(), "Высокое");
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// All texts of the tab for all desktops — as the buyer sees them.
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

/// The tab's English interface: not a single Russian letter — a forgotten string
/// shows up here, not to the buyer.
///
/// The exception is named: the desktop INSTALL texts (`install_texts`) are taken from
/// `qt_desktops` — that is the «Desktops» tab's pass, they are not included here.
#[test]
fn english_compare_has_no_russian() {
    let _lang = i18n_pin::en();
    let all = all_texts();
    assert!(all.len() > 200, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// English phrases read as phrases, and the load colour was not lost with
/// the label's translation.
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

/// The load colour is the same in any language: the feature decides, not the label.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
