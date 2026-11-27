//! Golden check of the `ui_base.rs` port against the original `rexon/ui_base.py`.
//!
//! The reference was captured read-only from the LIVE `rexon.ui_base` by a
//! one-off generator: Qt was not started (without a QApplication
//! `is_dark_theme` goes into its except branch), dark and light modes were
//! captured by replacing that function. Writing the SVG checkmark into
//! `~/.cache` was replaced by a stub — the file TEXT and path are checked, not
//! the file itself (I/O stays with the executor).
//!
//! Covered: text colours of both themes, text QSS fragments, `neutral_rgba`,
//! rich-text detection and escaping, scrollbar QSS of both themes, Synthwave
//! and «Appearance» tab tokens, the skin table (labels, descriptions,
//! palettes, tokens), the whole Synthwave QSS overlay and the title outline.
//!
//! `golden/ui_base_cases.json` — the captured values.

use rexon_core::ui_base as ui;
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/ui_base_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn text_colors() {
    let g = fx();
    let d = &g["dark_colors"];
    assert_eq!(ui::DARK_PRIMARY, s(&d["primary"]));
    assert_eq!(ui::DARK_SECONDARY, s(&d["secondary"]));
    assert_eq!(ui::DARK_MUTED, s(&d["muted"]));
    let l = &g["light_colors"];
    assert_eq!(ui::LIGHT_PRIMARY, s(&l["primary"]));
    assert_eq!(ui::LIGHT_SECONDARY, s(&l["secondary"]));
    assert_eq!(ui::LIGHT_MUTED, s(&l["muted"]));

    // Values returned by the original's functions themselves on both themes.
    let dark = g["dark_text_colors_fn"].as_array().unwrap();
    assert_eq!(ui::primary_text_color(true), s(&dark[0]));
    assert_eq!(ui::secondary_text_color(true), s(&dark[1]));
    assert_eq!(ui::muted_text_color(true), s(&dark[2]));
    let light = g["light_text_colors_fn"].as_array().unwrap();
    assert_eq!(ui::primary_text_color(false), s(&light[0]));
    assert_eq!(ui::secondary_text_color(false), s(&light[1]));
    assert_eq!(ui::muted_text_color(false), s(&light[2]));
}

#[test]
fn text_qss_fragments() {
    let g = fx();
    let q = &g["text_qss"];
    assert_eq!(ui::secondary_text_qss(""), s(&q["secondary"]));
    assert_eq!(
        ui::secondary_text_qss("font-size: 11pt;"),
        s(&q["secondary_extra"])
    );
    assert_eq!(ui::muted_text_qss(""), s(&q["muted"]));
    assert_eq!(ui::muted_text_qss("padding: 2px;"), s(&q["muted_extra"]));
    assert_eq!(ui::primary_text_qss(""), s(&q["primary"]));
    assert_eq!(
        ui::primary_text_qss("font-size: 13pt;"),
        s(&q["primary_extra"])
    );
}

#[test]
fn neutral_rgba_matches_python_g_format() {
    let g = fx();
    for (key, want) in g["neutral_rgba"].as_object().unwrap() {
        let alpha: f64 = key.parse().unwrap();
        assert_eq!(ui::neutral_rgba(alpha), s(want), "alpha={alpha}");
    }
}

#[test]
fn rich_text_detect() {
    let g = fx();
    for (text, want) in g["rich_text"].as_object().unwrap() {
        assert_eq!(
            ui::looks_like_rich_text(text),
            want.as_bool().unwrap(),
            "текст {text:?}"
        );
    }
}

#[test]
fn escape_dialog() {
    let g = fx();
    for (text, want) in g["escape"].as_object().unwrap() {
        assert_eq!(ui::escape_dialog_text(text), s(want), "текст {text:?}");
    }
}

/// The original accepts ANY object and calls `str()` before escaping. In the
/// port the coercion is pinned by the argument type — the caller converts, and
/// the escaping of the string itself must match the reference.
#[test]
fn escape_dialog_after_python_str() {
    let g = fx();
    let want = g["escape_nonstr"].as_array().unwrap();
    // Exactly what Python's `str()` gives on None / 42 / 3.5 / a list.
    for (stringified, expect) in
        ["None", "42", "3.5", "['a', '<b>']"].iter().zip(want)
    {
        assert_eq!(ui::escape_dialog_text(stringified), s(expect));
    }
}

#[test]
fn scrollbar() {
    let g = fx();
    assert_eq!(ui::scrollbar_qss("transparent", true), s(&g["scrollbar_dark"]));
    assert_eq!(
        ui::scrollbar_qss("#1b2838", true),
        s(&g["scrollbar_dark_track"])
    );
    assert_eq!(
        ui::scrollbar_qss("transparent", false),
        s(&g["scrollbar_light"])
    );
    assert_eq!(
        ui::scrollbar_qss("#f0f0f0", false),
        s(&g["scrollbar_light_track"])
    );
}

#[test]
fn indicator_check() {
    let g = fx();
    for (color, want) in g["indicator_svg"].as_object().unwrap() {
        assert_eq!(ui::indicator_check_svg(color), s(want), "цвет {color}");
    }
    // File name: the hash is dropped (like the original's `color.lstrip('#')`).
    assert_eq!(
        ui::indicator_check_filename("#0d1b2a"),
        "indicator-check-0d1b2a.svg"
    );
    assert_eq!(
        ui::indicator_check_filename("0d1b2a"),
        "indicator-check-0d1b2a.svg"
    );
}

#[test]
fn tokens() {
    let g = fx();
    for (key, want) in g["sw_tokens"].as_object().unwrap() {
        let got = ui::SW_TOKENS
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("нет токена {key}"));
        assert_eq!(got.1, s(want), "токен {key}");
    }
    assert_eq!(ui::SW_TOKENS.len(), g["sw_tokens"].as_object().unwrap().len());

    for (key, want) in g["appearance_steam_colors"].as_object().unwrap() {
        let got = ui::APPEARANCE_STEAM_COLORS
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("нет токена {key}"));
        assert_eq!(got.1, s(want), "токен {key}");
    }
}

#[test]
fn skins_table() {
    // The reference is the original's Russian labels, checked via po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ui::DEFAULT_SKIN, s(&g["default_skin"]));

    let want: Vec<(String, String)> = g["available_skins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (s(&a[0]).to_string(), s(&a[1]).to_string())
        })
        .collect();
    let got: Vec<(String, String)> = ui::available_skins()
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    assert_eq!(got, want, "порядок и метки скинов");

    for (id, desc) in g["skin_desc"].as_object().unwrap() {
        assert_eq!(ui::skin_description(id), s(desc), "описание {id}");
    }
    for (id, valid) in g["is_valid"].as_object().unwrap() {
        assert_eq!(ui::is_valid_skin(id), valid.as_bool().unwrap(), "валидность {id}");
    }
    for (id, colors) in g["skin_colors"].as_object().unwrap() {
        let got = ui::skin_colors(id);
        let want = colors.as_object().unwrap();
        assert_eq!(got.len(), want.len(), "число токенов скина {id}");
        for (k, v) in want {
            assert_eq!(got.get(k).map(String::as_str), Some(s(v)), "{id}.{k}");
        }
    }
    for (id, pal) in g["skin_palette"].as_object().unwrap() {
        match (ui::skin_palette(id), pal) {
            (None, Value::Null) => {}
            (Some(got), Value::Object(want)) => {
                assert_eq!(got.len(), want.len(), "число ролей палитры {id}");
                for (role, hex) in want {
                    let found = got.iter().find(|(k, _)| k == role);
                    assert_eq!(found.map(|(_, v)| *v), Some(s(hex)), "{id}.{role}");
                }
            }
            (got, want) => panic!("палитра {id} разошлась: {got:?} против {want}"),
        }
    }
}

#[test]
fn synthwave_overlay() {
    let g = fx();
    // The reference was captured with a stub for the checkmark path — we substitute the same token.
    assert_eq!(ui::synthwave_qss("<CHECK>"), s(&g["synthwave_qss"]));
    for (id, qss) in g["skin_qss"].as_object().unwrap() {
        assert_eq!(ui::skin_qss(id, "<CHECK>"), s(qss), "QSS скина {id}");
    }
}

#[test]
fn heading_outline() {
    let g = fx();
    for (id, want) in g["heading_outline"].as_object().unwrap() {
        match (ui::heading_outline_colors(id), want) {
            (None, Value::Null) => {}
            (Some((stroke, fill, width)), Value::Array(a)) => {
                assert_eq!(stroke, s(&a[0]), "обводка {id}");
                assert_eq!(fill, s(&a[1]), "заливка {id}");
                assert_eq!(u64::from(width), a[2].as_u64().unwrap(), "ширина {id}");
            }
            (got, w) => panic!("обводка {id} разошлась: {got:?} против {w}"),
        }
    }
}

#[test]
fn toast_qss_both_themes() {
    let g = fx()["toast_qss"].clone();
    assert_eq!(ui::toast_qss(true), s(&g["dark"]));
    assert_eq!(ui::toast_qss(false), s(&g["light"]));
}

/// The active skin: the «unknown name → default skin» fork is one for the
/// product. Before this TWO copies held it — `ui_base.py::_current_skin_id`
/// for the widget window and a baked-in `DEFAULT_SKIN` in four QML pages; the
/// second copy always answered «steam_dark», and the migrated tabs stayed dark
/// blue for a buyer who had chosen «Light».
#[test]
fn active_skin_falls_back_and_follows_choice() {
    assert_eq!(ui::active_skin_id_from(None), ui::DEFAULT_SKIN, "нет настройки");
    assert_eq!(ui::active_skin_id_from(Some("")), ui::DEFAULT_SKIN, "пусто");
    assert_eq!(
        ui::active_skin_id_from(Some("нет-такого")),
        ui::DEFAULT_SKIN,
        "незнакомое имя"
    );
    for (id, _) in ui::available_skins() {
        assert_eq!(ui::active_skin_id_from(Some(id)), id, "выбранный скин {id}");
    }
}

/// The skins really ARE DIFFERENT: if the tokens matched, replacing
/// `skin_colors(DEFAULT_SKIN)` with the active skin would change nothing, and
/// the check above would be guarding emptiness.
#[test]
fn skins_differ_in_colors() {
    let base = ui::skin_colors(ui::DEFAULT_SKIN);
    for id in ["light", "synthwave"] {
        let other = ui::skin_colors(id);
        assert_ne!(other, base, "скин {id} обязан отличаться от стандартного");
        assert_eq!(other.len(), base.len(), "набор ключей скина {id}");
    }
    // «System» imposes nothing over the system palette — its tokens deliberately
    // match the standard ones.
    assert_eq!(ui::skin_colors("system"), base, "системный скин");
}

/// Wiring guard: NOT A SINGLE QML page is painted with the baked-in name of
/// the standard skin. The check is general — over all `*_page.rs` of the
/// crate, not over four known names: the next migrated tab falls under it by
/// itself (rule #12), and it can no longer silently drift away from the
/// widget window.
#[test]
fn qml_pages_take_the_active_skin() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/src");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("нет каталога исходников rexon-gui") {
        let path = entry.expect("запись каталога").path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with("_page.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("страница не читается");
        if !src.contains("fn palette(") {
            continue;
        }
        checked += 1;
        assert!(
            !src.contains("skin_colors(ui_base::DEFAULT_SKIN)"),
            "{name}: палитра берётся у стандартного скина вместо выбранного"
        );
        // ⚠️Exactly `active_qml_palette`, not `active_skin_colors`: besides
        // the skin tokens the page must get READY surface paints
        // (`surface_field`, `surface_card`). While the pages took only
        // tokens, each one picked a token that looked similar to the eye
        // for the surface and drifted from the widget original by one
        // paint step.
        assert!(
            src.contains("ui_base::active_qml_palette()"),
            "{name}: палитра собрана мимо общего ответа active_qml_palette"
        );
    }
    assert!(checked >= 4, "проверено страниц: {checked} — сторож смотрит мимо");
}

/// Lightening the accent is the shared hover technique: skins have no «hovered»
/// shade of their own, and with four hand-written values a fifth skin would be
/// bypassed again.
#[test]
fn lighten_mixes_with_white() {
    assert_eq!(ui::lighten_hex("#000000", 0.0), "#000000", "нулевая доля");
    assert_eq!(ui::lighten_hex("#000000", 1.0), "#ffffff", "полная доля");
    assert_eq!(ui::lighten_hex("#66c0f4", 0.25), "#8cd0f7", "акцент steam_dark");
    assert_eq!(ui::lighten_hex("#ffffff", 0.5), "#ffffff", "белее белого нет");
    // A fraction outside 0..1 does not push the colour beyond the channel bounds.
    assert_eq!(ui::lighten_hex("#66c0f4", 5.0), "#ffffff", "доля выше единицы");
    assert_eq!(ui::lighten_hex("#66c0f4", -1.0), "#66c0f4", "доля ниже нуля");
    // An unparsed colour is returned as is — hover does not kill the button.
    assert_eq!(ui::lighten_hex("transparent", 0.25), "transparent");
}

/// Wiring guard #2: EVERY QML page is repainted on the fly.
///
/// The previous fix (3ee6011) taught the pages to take the colours of the
/// chosen skin, but the `palette` property stayed `CONSTANT` — there was no
/// change signal at all, bindings were not re-evaluated, and an open tab kept
/// the colours of the previous skin while the widget window was already
/// repainted. Half the window in the new skin, half in the old — the buyer
/// considers it a breakage (#13).
///
/// The check is general, over all `*_page.rs` of the crate: the broadcast
/// mechanism is ONE (`rexon_gui::skin`), and the next migrated tab must
/// subscribe with the same two lines rather than start a fifth copy of the
/// broadcast (rule #12). That the repaint reaches the LIVE object is proved by
/// the measurement `scripts/test_qml_skin_live.py` — only the wiring is guarded here.
#[test]
fn qml_pages_repaint_on_skin_change() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/src");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("нет каталога исходников rexon-gui") {
        let path = entry.expect("запись каталога").path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with("_page.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("страница не читается");
        if !src.contains("fn palette(") {
            continue;
        }
        checked += 1;
        assert!(
            !src.contains("palette, READ = palette, CONSTANT"),
            "{name}: палитра объявлена CONSTANT — смена скина её не перекрасит"
        );
        assert!(
            src.contains("NOTIFY = palette_changed"),
            "{name}: у палитры нет сигнала об изменении"
        );
        assert!(
            src.contains("crate::skin::watch("),
            "{name}: страница не подписана на общую рассылку смены скина"
        );
        assert!(
            src.contains("crate::skin::unwatch("),
            "{name}: подписка на смену скина не снимается при закрытии вкладки"
        );
    }
    assert!(checked >= 4, "проверено страниц: {checked} — сторож смотрит мимо");
}

/// Translating a paint from QSS notation into one QML understands.
///
/// ⛔WHY THE GUARD. `rgba(...)` is a legitimate notation for widget QSS and
/// an INVALID one for QML: a page paints black with such a property without
/// saying a word. That is how card badges, the large status chip and the
/// stars on the «Steam» tab came out black — the defect lived until the
/// pixel-by-pixel comparison with the original on 01.09.2026.
#[test]
fn css_color_to_qml_переводит_rgba() {
    use rexon_core::ui_base::css_color_to_qml;

    // ⚠️Alpha 0.85 → 0xd8, NOT 0xd9: Qt's style sheet parser truncates the
    // fraction (0.85 * 255 = 216.75 → 216). While rounding stood here, the paint
    // arrived in QML one step denser than the widget one — see the guard
    // `доля_прозрачности_усекается_как_у_разбора_qss`.
    assert_eq!(css_color_to_qml("rgba(59,130,246,0.85)"), "#d83b82f6");
    assert_eq!(css_color_to_qml("rgba(127,127,127,0.14)"), "#237f7f7f");
    // Fully opaque — alpha ff.
    assert_eq!(css_color_to_qml("rgba(234,179,8,1.0)"), "#ffeab308");
    // Without alpha.
    assert_eq!(css_color_to_qml("rgb(34,197,94)"), "#ff22c55e");
    // Spaces in the notation are legitimate.
    assert_eq!(css_color_to_qml("rgba( 0, 0, 0, 0.45 )"), "#72000000");
    // What QML understands by itself — we leave alone.
    for as_is in ["#1a1a1a", "white", "transparent", ""] {
        assert_eq!(css_color_to_qml(as_is), as_is);
    }
    // Garbage is returned as is: inventing a colour instead of an error is not allowed.
    assert_eq!(css_color_to_qml("rgba(1,2)"), "rgba(1,2)");
    assert_eq!(css_color_to_qml("rgba(1,2,300,0.5)"), "rgba(1,2,300,0.5)");
}

/// All paints of the «Steam» tab that go into QML must be translated.
#[test]
fn краски_steam_переводятся_для_qml() {
    use rexon_core::qt_steam as qs;
    use rexon_core::ui_base::css_color_to_qml;

    let mut colors: Vec<String> = Vec::new();
    for dark in [true, false] {
        let (bg, border) = qs::group_colors(dark);
        colors.extend([bg, border]);
        for selected in [true, false] {
            let (a, b, c) = qs::card_colors(selected, dark);
            colors.extend([a, b, c]);
        }
        for tone in [qs::BADGE_NEUTRAL, qs::BADGE_ACCENT, qs::BADGE_GOLD] {
            let (bg, _) = qs::badge_colors(tone, dark);
            colors.push(bg);
        }
        for tone in [qs::CHIP_OK, qs::CHIP_WARN, qs::CHIP_OFF] {
            let (a, b, c) = qs::chip_colors(tone, dark);
            colors.extend([a, b, c]);
        }
    }
    let (a, b, c, _) = qs::primary_btn_colors();
    colors.extend([a, b, c]);
    let (a, b, _) = qs::applied_btn_colors();
    colors.extend([a, b]);
    let (a, b, c, d) = qs::switch_chip_colors();
    colors.extend([a, b, c, d]);
    let (a, b) = qs::pitch_colors();
    colors.extend([a, b]);
    colors.extend([
        qs::star_color(true),
        qs::star_color(false),
        qs::SV_THUMB_BG.to_string(),
        qs::BG_THUMB_BG.to_string(),
        qs::HERO_THUMB_BG.to_string(),
    ]);

    for c in colors {
        let qml = css_color_to_qml(&c);
        assert!(
            qml.starts_with('#') || qml == "white",
            "краска {c:?} не переводится в вид QML (вышло {qml:?}) — \
             страница нарисует её чёрной"
        );
    }
}

// ============================================================================
// Skin surfaces for QML: the paint arrives READY, not as a fraction
// ============================================================================

/// Parsing the opacity fraction TRUNCATES, as Qt's style sheet parser does.
///
/// ⛔WHY THE GUARD. The widget part is the reference, and Qt gets its paints
/// from a style sheet, where the fraction is truncated: the experiment of
/// 04.09 on a `#1b2838` background (a fraction against an integer alpha
/// «down» and «up») gave for 0.85 a paint of alpha 216, for 0.45 — 114, for
/// 0.14 — 35, for 0.25 — 63, for 0.03 — 7, for 0.12 — 30. While the
/// translator rounded, EVERY semi-transparent paint arrived in QML one step
/// denser than the widget one, i.e. on a dark background one step lighter.
/// The stand threshold (24 levels) did not show such a divergence: the class
/// was caught by name twice (2916fcc, 5ac5d42) without seeing the common root.
#[test]
fn доля_прозрачности_усекается_как_у_разбора_qss() {
    use rexon_core::ui_base::{css_color_to_qml, parse_css_rgba};

    for (доля, альфа) in [
        ("0.85", 216u8),
        ("0.45", 114),
        ("0.14", 35),
        ("0.25", 63),
        ("0.03", 7),
        ("0.12", 30),
        ("0.04", 10),
        ("1.0", 255),
        ("0", 0),
    ] {
        let css = format!("rgba(255,255,255,{доля})");
        assert_eq!(
            parse_css_rgba(&css),
            Some((255, 255, 255, альфа)),
            "долю {доля} разбор таблиц стилей Qt усекает до {альфа}"
        );
        assert_eq!(css_color_to_qml(&css), format!("#{альфа:02x}ffffff"));
    }
}

/// An overlay laid on a paint gives the same as Qt draws on screen.
#[test]
fn накладка_смешивается_как_на_экране() {
    use rexon_core::ui_base::{blend_css_over, CARD_FILL_DARK};

    // Measurement of 04.09: a white overlay on `#1b2838` gives for widgets
    // (36,48,64) at fraction 0.04, (54,65,79) at 0.12 and (58,70,83) at 0.14.
    assert_eq!(blend_css_over("#1b2838", CARD_FILL_DARK), "#243040");
    assert_eq!(blend_css_over("#1b2838", "rgba(255,255,255,0.12)"), "#36414f");
    assert_eq!(blend_css_over("#1b2838", "rgba(255,255,255,0.14)"), "#3a4653");
    // An opaque overlay covers the base entirely.
    assert_eq!(blend_css_over("#1b2838", "rgba(102,192,244,1.0)"), "#66c0f4");
    // Nothing to parse — the notation is returned as is.
    assert_eq!(blend_css_over("#1b2838", "white"), "white");
}

/// ⛔CLASS GUARD. Any skin SURFACE given to QML must arrive as a READY PAINT,
/// not as a fraction and not as QSS notation.
///
/// WHY. QML markup knows neither Qt palette roles nor the style sheet cascade,
/// and the pages picked a token that looked similar to the eye: the input
/// field was painted `bg_card` (30,45,61) where Qt paints the `Base` role
/// (22,32,45), and the card — with the same `bg_card` where the widget style
/// sheet lays a 0.04 overlay on the page background (36,48,64). Both paints
/// are now built entirely by the core. The guard makes sure the surface does
/// not come back as a fraction or as `rgba(...)` notation: QML reads both in
/// its own way.
#[test]
fn поверхности_скина_приходят_в_qml_готовой_краской() {
    use rexon_core::ui_base::{parse_hex, qml_palette, skin_surface_colors, SKINS};

    for skin in SKINS {
        let surfaces = skin_surface_colors(skin.id);
        assert!(
            surfaces.contains_key("surface_field") && surfaces.contains_key("surface_card"),
            "скин {} остался без поверхностей",
            skin.id
        );
        for (key, value) in &surfaces {
            assert!(
                parse_hex(value).is_some(),
                "поверхность {key} скина {} отдана не готовой краской, а {value:?}",
                skin.id
            );
        }
        // The page palette must carry the surfaces next to the tokens.
        let pal = qml_palette(skin.id);
        for (key, value) in &surfaces {
            assert_eq!(pal.get(key), Some(value), "{key} потерян в палитре QML");
        }
    }
}

/// The field surface is the skin's `Base` ROLE, the same one Qt paints
/// `QLineEdit` and `QComboBox` with in the widget part.
///
/// Measurement of 04.09 (experiment on live Qt): a `QLineEdit` with the
/// `Base` role `#16202d` draws (22,32,45) — exactly what the stand captured
/// from the widget «Extensions» tab; a `bg_card` `#1e2d3d` fill in a QML scene
/// gives (30,45,61), exactly what the stand captured from the QML tab on
/// 37 585 points of the search field.
#[test]
fn поверхность_поля_равна_роли_base() {
    use rexon_core::ui_base::{skin_role_color, skin_surface_colors, SKINS};

    for skin in SKINS {
        let Some(base) = skin_role_color(skin.id, "Base") else {
            continue; // system skin: roles stay system ones
        };
        assert_eq!(
            skin_surface_colors(skin.id).get("surface_field").map(String::as_str),
            Some(base),
            "поверхность поля скина {} разошлась с ролью Base",
            skin.id
        );
    }
    assert_eq!(
        skin_surface_colors("steam_dark").get("surface_field").map(String::as_str),
        Some("#16202d")
    );
    // A button and a non-editable drop-down list — another role, `Button`.
    for skin in SKINS {
        let Some(button) = skin_role_color(skin.id, "Button") else {
            continue;
        };
        assert_eq!(
            skin_surface_colors(skin.id).get("surface_button").map(String::as_str),
            Some(button),
            "поверхность кнопки скина {} разошлась с ролью Button",
            skin.id
        );
    }
    assert_eq!(
        skin_surface_colors("steam_dark").get("surface_button").map(String::as_str),
        Some("#2a4055")
    );
}

/// The outline of a button and a field is a Qt style overlay, not a palette role.
///
/// ⚠️ THE NUMBERS WERE TAKEN FROM LIVE Qt, not derived (measurement of 05.09,
/// skin steam_dark): the «Quick pairs» button of «Compare» is filled
/// (42,64,85) by the `Button` role and outlined (85,102,119); the
/// «Extensions» search field is filled (22,32,45) by the `Base` role and
/// outlined (73,83,96) — with an overlay over the page BACKGROUND (27,40,56),
/// where the field fill does not reach. While the QML markup took the
/// `border` token (42,64,85) for both frames, the field diverged from the
/// original by 31 levels, and the button had no frame at all.
#[test]
fn control_outline_is_style_overlay() {
    use rexon_core::ui_base::{
        blend_css_over, skin_surface_colors, CONTROL_OUTLINE_DARK, CONTROL_OUTLINE_LIGHT, SKINS,
    };

    let surfaces = skin_surface_colors("steam_dark");
    assert_eq!(
        surfaces.get("surface_button_outline").map(String::as_str),
        Some("#556677"),
        "обводка кнопки steam_dark разошлась с замером (85,102,119)"
    );
    assert_eq!(
        surfaces.get("surface_field_outline").map(String::as_str),
        Some("#495360"),
        "обводка поля steam_dark разошлась с замером (73,83,96)"
    );

    // For every skin the outline is built with THE SAME overlay: the button —
    // over its fill, the field — over the page background. Which of the two
    // overlays (light or dark) is decided by the background lightness, so any works.
    for skin in SKINS {
        let surfaces = skin_surface_colors(skin.id);
        let page = surfaces.get("surface_page").cloned().unwrap_or_default();
        let button = surfaces.get("surface_button").cloned().unwrap_or_default();
        let button_outline = surfaces
            .get("surface_button_outline")
            .cloned()
            .unwrap_or_default();
        let field_outline = surfaces
            .get("surface_field_outline")
            .cloned()
            .unwrap_or_default();
        assert!(
            button_outline == blend_css_over(&button, CONTROL_OUTLINE_DARK)
                || button_outline == blend_css_over(&button, CONTROL_OUTLINE_LIGHT),
            "обводка кнопки скина {} собрана не накладкой стиля",
            skin.id
        );
        assert!(
            field_outline == blend_css_over(&page, CONTROL_OUTLINE_DARK)
                || field_outline == blend_css_over(&page, CONTROL_OUTLINE_LIGHT),
            "обводка поля скина {} собрана не накладкой стиля",
            skin.id
        );
    }
}

/// The card surface is built with THE SAME overlay as in the widget card's
/// style sheet — otherwise the two paints would silently diverge.
#[test]
fn поверхность_карточки_равна_накладке_виджетов() {
    use rexon_core::qt_compare;
    use rexon_core::qt_steam;
    use rexon_core::ui_base::{
        blend_css_over, skin_colors, skin_surface_colors, CARD_FILL_DARK, CARD_FILL_LIGHT,
    };

    // The overlay literal of the Steam showcase (and through it of «Extensions»)
    // is the same as in the surface.
    assert_eq!(qt_steam::card_colors(false, true).0, CARD_FILL_DARK);
    assert_eq!(qt_steam::card_colors(false, false).0, CARD_FILL_LIGHT);
    // And for «Compare»: its card style sheet is written with the same one.
    assert!(qt_compare::card_qss("deCard", true).contains(CARD_FILL_DARK));
    assert!(qt_compare::card_qss("deCard", false).contains(CARD_FILL_LIGHT));

    // Standard skin: a 0.04 overlay on `#1b2838` is (36,48,64), exactly what the
    // stand captured from the widget card of «Compare».
    assert_eq!(
        skin_surface_colors("steam_dark").get("surface_card").map(String::as_str),
        Some("#243040")
    );
    // The light skin takes the light overlay — otherwise the card is «lighter than white».
    let light_page = skin_colors("light")["bg_primary"].clone();
    assert_eq!(
        skin_surface_colors("light").get("surface_card").map(String::as_str),
        Some(blend_css_over(&light_page, CARD_FILL_LIGHT).as_str())
    );
}

/// Disabled QML text is painted with THE SAME paint the skin plan writes into
/// the Qt palette — otherwise a disabled button fades differently in the two versions.
///
/// WHY. The markup faded a disabled button with 0.45 opacity on the whole
/// button: for widgets the fill and frame stay the same, and only the label
/// is dimmed. Measurement of 05.09 on «Extensions» (the «Steam Workshop»
/// button of the «Install» block): 10 108 points, (36,48,64) versus (31,43,59).
#[test]
fn неактивный_текст_qml_равен_группе_disabled_палитры() {
    use rexon_core::ui_base::{
        disabled_text_color, palette_plan, qml_palette, DISABLED_TEXT_ALPHA, SKINS,
    };

    for skin in SKINS {
        let value = disabled_text_color(skin.id);
        assert_eq!(value.len(), 9, "скин {}: {value} не #aarrggbb", skin.id);

        // Exactly the assignment the executor puts into QPalette.
        let plan = palette_plan(skin.id, true);
        let assign = plan
            .iter()
            .rev()
            .find(|a| a.group == "Disabled" && a.role == "ButtonText")
            .unwrap_or_else(|| panic!("скин {}: план без Disabled/ButtonText", skin.id));
        let (r, g, b, a) = assign.rgba;
        assert_eq!(
            value,
            format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
            "скин {}: краска QML разошлась с планом палитры",
            skin.id
        );
        assert_eq!(a, DISABLED_TEXT_ALPHA, "скин {}", skin.id);

        // The page palette must carry it: the markup has nowhere else to take it from.
        assert_eq!(
            qml_palette(skin.id).get("text_disabled"),
            Some(&value),
            "скин {}: text_disabled потерян в палитре QML",
            skin.id
        );
    }

    // Standard skin: white with opacity 140 — the same step that truncating the
    // 0.55 fraction gives in the tab style sheets.
    assert_eq!(disabled_text_color("steam_dark"), "#8cffffff");
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Raw network errors for all six reasons — with and without a directory name.
const NETWORK_SAMPLES: &[&str] = &[
    "<urlopen error timed out>",
    "Name or service not known",
    "Network Error: Unexpected EOF",
    "Network is unreachable",
    "HTTP Error 503: Service Unavailable",
    "нечто непонятное",
];

/// Skins in «Settings» in English: labels and descriptions are translated, the
/// identifiers (`id`) are the same as the `ui_skin` setting writes, so a choice
/// saved with the Russian interface is recognised with the English one too.
#[test]
fn english_skins_have_no_russian() {
    let _lang = i18n_pin::en();
    let got = ui::available_skins();
    let want = [
        ("system", "System (follow the system theme)"),
        ("steam_dark", "Dark (Steam)"),
        ("light", "Light"),
        ("synthwave", "Synthwave / Neon Space Retro"),
        ("waywallen", "Dark (Material)"),
    ];
    assert_eq!(got.len(), want.len());
    for ((id, label), (wid, wlabel)) in got.iter().zip(want) {
        assert_eq!((*id, label.as_str()), (wid, wlabel));
        assert!(ui::is_valid_skin(id), "признак {id} перестал узнаваться");
        assert_eq!(ui::active_skin_id_from(Some(id)), *id);
        let desc = ui::skin_description(id);
        assert!(!desc.is_empty() && !has_cyrillic(&desc), "{id}: {desc}");
    }
    assert_eq!(
        ui::skin_description("steam_dark"),
        "Dark blue colors in the style of the Steam client. The default."
    );
    for raw in NETWORK_SAMPLES {
        for source in ["AUR", ""] {
            let text = ui::humanize_network_error(raw, source);
            assert!(text.starts_with("⚠️ ") && !has_cyrillic(&text), "{raw} / {source}: {text}");
            assert_eq!(text.contains("AUR"), !source.is_empty(), "{text}");
        }
    }
    assert_eq!(
        ui::humanize_network_error("HTTP Error 503", "AUR"),
        "⚠️ The AUR catalog returned error 503. This is most likely temporary — try again later."
    );
}

/// Skin labels and network errors in Russian — word for word as before the
/// translation (the grammatical cases of the word «directory» as whole phrases).
#[test]
fn russian_skins_and_network_errors_read_as_before() {
    let _lang = i18n_pin::ru();
    let labels: Vec<(&str, String)> = ui::available_skins();
    assert_eq!(
        labels,
        vec![
            ("system", "Системный (как в системе)".to_string()),
            ("steam_dark", "Тёмный (Steam)".to_string()),
            ("light", "Светлый".to_string()),
            ("synthwave", "Synthwave / Neon Space Retro".to_string()),
            ("waywallen", "Тёмный (Material)".to_string()),
        ]
    );
    assert_eq!(
        ui::skin_description("synthwave"),
        "Неон-ретро: градиент фиолет→синий, акценты розовый/зелёный/голубой, неон-рамки и \
         скругления. Свечение приближено (Qt QSS не поддерживает box-shadow)."
    );
    let want: &[(&str, &str, &str)] = &[
        (
            NETWORK_SAMPLES[0],
            "⚠️ Каталог AUR не ответил вовремя. Возможно, сервис перегружен — попробуйте ещё раз.",
            "⚠️ Каталог не ответил вовремя. Возможно, сервис перегружен — попробуйте ещё раз.",
        ),
        (
            NETWORK_SAMPLES[1],
            "⚠️ Не удалось найти адрес каталога AUR. Проверьте подключение к интернету и настройки DNS.",
            "⚠️ Не удалось найти адрес каталога. Проверьте подключение к интернету и настройки DNS.",
        ),
        (
            NETWORK_SAMPLES[2],
            "⚠️ Соединение с каталогом AUR прервалось. Так бывает при обрыве связи, а также если \
             доступ к нему закрыт брандмауэром, VPN или провайдером.",
            "⚠️ Соединение с каталогом прервалось. Так бывает при обрыве связи, а также если \
             доступ к нему закрыт брандмауэром, VPN или провайдером.",
        ),
        (
            NETWORK_SAMPLES[3],
            "⚠️ Сеть недоступна — не получилось связаться с каталогом AUR.",
            "⚠️ Сеть недоступна — не получилось связаться с каталогом.",
        ),
        (
            NETWORK_SAMPLES[4],
            "⚠️ Каталог AUR ответил ошибкой 503. Скорее всего, это временно — попробуйте позже.",
            "⚠️ Каталог ответил ошибкой 503. Скорее всего, это временно — попробуйте позже.",
        ),
        (
            NETWORK_SAMPLES[5],
            "⚠️ Не удалось связаться с каталогом AUR. Проверьте подключение к интернету и попробуйте ещё раз.",
            "⚠️ Не удалось связаться с каталогом. Проверьте подключение к интернету и попробуйте ещё раз.",
        ),
    ];
    for (raw, named, plain) in want {
        assert_eq!(ui::humanize_network_error(raw, "AUR"), *named, "{raw}");
        assert_eq!(ui::humanize_network_error(raw, ""), *plain, "{raw}");
    }
}

/// Palette keys read by the shared `RexonSlider` must be in the SHARED palette
/// of every skin: the slider also sits in «Appearance» sections (Conky,
/// «Screen Effects», «Animation») whose page adds only its own entries to the
/// palette. On 08.11.2026 the `track` key was missing there — «Unable to
/// assign [undefined] to QColor» when opening the tab.
#[test]
fn slider_keys_are_in_every_skin_palette() {
    let slider = include_str!("../../rexon-gui/qml/RexonSlider.qml");
    let mut keys: Vec<String> = Vec::new();
    for part in slider.split("control.pal.").skip(1) {
        let key: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    assert!(keys.contains(&"track".to_string()), "сторож не нашёл чтение pal.track: {keys:?}");
    for skin in ui::SKINS {
        let pal = ui::qml_palette(skin.id);
        for key in &keys {
            let value = pal.get(key).unwrap_or_else(|| panic!("скин {}: в палитре нет «{key}»", skin.id));
            assert!(!value.starts_with("rgba("), "скин {}: «{key}» записан по-QSS: {value}", skin.id);
        }
        assert_eq!(pal["track"], ui::css_color_to_qml(&ui::neutral_rgba(ui::SLIDER_TRACK_ALPHA)));
    }
}

/// Scrolling to the focused element does not fire on the window regaining activity.
///
/// ⛔WHY THE GUARD. Defect of 08.11.2026 (MATE X11, «Settings» tab): scrolled
/// the page down with the wheel, pressed Print — mate-screenshot opened its
/// dialog, the Rexon window lost activity, and when the dialog closed Qt
/// returned focus to the same element («Language» at the top). Its
/// `onActiveFocusChanged` called `ensureVisible`, and the page jumped to the
/// top. Focus returned by the window comes with the reason
/// `Qt.ActiveWindowFocusReason` — every `ensureVisible` must skip it. The
/// check is general, over all pages: the next tab with the same scrolling
/// will fall under it by itself.
#[test]
fn qml_ensure_visible_ignores_window_activation() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/qml");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("нет каталога QML rexon-gui") {
        let path = entry.expect("запись каталога").path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".qml") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("QML не читается");
        for (at, _) in src.match_indices("function ensureVisible(") {
            checked += 1;
            // The function body — up to the first closing brace at the function level.
            let body = &src[at..];
            let end = body.find("\n    }\n").unwrap_or(body.len());
            assert!(
                body[..end].contains("Qt.ActiveWindowFocusReason"),
                "{name}: ensureVisible крутит страницу и на возврат активности окна"
            );
        }
    }
    assert!(checked >= 4, "проверено подкруток: {checked} — сторож смотрит мимо");
}
