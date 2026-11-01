//! Golden-сверка порта `ui_base.rs` с оригиналом `rexon/ui_base.py`.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.ui_base` одноразовым генератором:
//! Qt не поднимался (без QApplication `is_dark_theme` уходит в свою
//! except-ветку), тёмный и светлый режимы сняты подменой этой функции.
//! Запись SVG-галочки в `~/.cache` подменена заглушкой — сверяется ТЕКСТ
//! файла и путь, но не сам файл (I/O остаётся исполнителю).
//!
//! Покрыто: цвета текста обеих тем, QSS-фрагменты текста, `neutral_rgba`,
//! детект и экранирование rich-text, QSS скроллбара обеих тем, токены
//! Synthwave и вкладки «Оформление», таблица скинов (метки, описания,
//! палитры, токены), QSS-оверлей Synthwave целиком и обводка заголовка.
//!
//! `golden/ui_base_cases.json` — снятые значения.

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

    // Значения, отданные самими функциями оригинала на обеих темах.
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

/// Оригинал принимает ЛЮБОЙ объект и зовёт `str()` перед экранированием.
/// В порте коэрция закреплена типом аргумента — приводит вызывающий, а
/// экранирование уже строки обязано совпадать с эталоном.
#[test]
fn escape_dialog_after_python_str() {
    let g = fx();
    let want = g["escape_nonstr"].as_array().unwrap();
    // Ровно то, что даёт `str()` Python на None / 42 / 3.5 / списке.
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
    // Имя файла: решётка отбрасывается (как `color.lstrip('#')` оригинала).
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
    // Эталон — русские подписи оригинала, сверка через po/ru.po.
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
    // Эталон снят с заглушкой пути галочки — подставляем тот же токен.
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

/// Действующий скин: развилка «незнакомое имя → скин по умолчанию» одна на
/// продукт. До этого её держали ДВЕ копии — `ui_base.py::_current_skin_id`
/// для окна на виджетах и запечённое `DEFAULT_SKIN` в четырёх страницах на
/// QML; вторая копия отвечала «steam_dark» всегда, и перенесённые вкладки
/// оставались тёмно-синими у покупателя, выбравшего «Светлый».
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

/// Скины действительно РАЗНЫЕ: если бы токены совпадали, замена
/// `skin_colors(DEFAULT_SKIN)` на действующий скин ничего бы не меняла, и
/// проверка выше сторожила бы пустоту.
#[test]
fn skins_differ_in_colors() {
    let base = ui::skin_colors(ui::DEFAULT_SKIN);
    for id in ["light", "synthwave"] {
        let other = ui::skin_colors(id);
        assert_ne!(other, base, "скин {id} обязан отличаться от стандартного");
        assert_eq!(other.len(), base.len(), "набор ключей скина {id}");
    }
    // «Системный» ничего не навязывает поверх системной палитры — его
    // токены намеренно совпадают со стандартными.
    assert_eq!(ui::skin_colors("system"), base, "системный скин");
}

/// Сторож проводки: НИ ОДНА страница на QML не красится запечённым именем
/// стандартного скина. Проверка общая — по всем `*_page.rs` крейта, а не по
/// четырём известным именам: следующая перенесённая вкладка попадает под
/// неё сама (правило #12), а разъехаться с окном на виджетах она молча уже
/// не сможет.
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
        // ⚠️Именно `active_qml_palette`, а не `active_skin_colors`: кроме
        // токенов скина страница обязана получить ГОТОВЫЕ краски
        // поверхностей (`surface_field`, `surface_card`). Пока страницы
        // брали одни токены, каждая выбирала для поверхности похожий на
        // глаз токен и разъезжалась с виджетным оригиналом на ступень
        // краски.
        assert!(
            src.contains("ui_base::active_qml_palette()"),
            "{name}: палитра собрана мимо общего ответа active_qml_palette"
        );
    }
    assert!(checked >= 4, "проверено страниц: {checked} — сторож смотрит мимо");
}

/// Осветление акцента — общий приём наведения: своего оттенка «наведено» у
/// скинов нет, и четырьмя ручными значениями пятый скин снова обошли бы.
#[test]
fn lighten_mixes_with_white() {
    assert_eq!(ui::lighten_hex("#000000", 0.0), "#000000", "нулевая доля");
    assert_eq!(ui::lighten_hex("#000000", 1.0), "#ffffff", "полная доля");
    assert_eq!(ui::lighten_hex("#66c0f4", 0.25), "#8cd0f7", "акцент steam_dark");
    assert_eq!(ui::lighten_hex("#ffffff", 0.5), "#ffffff", "белее белого нет");
    // Доля вне 0..1 не выводит цвет за границы каналов.
    assert_eq!(ui::lighten_hex("#66c0f4", 5.0), "#ffffff", "доля выше единицы");
    assert_eq!(ui::lighten_hex("#66c0f4", -1.0), "#66c0f4", "доля ниже нуля");
    // Неразобранный цвет возвращается как есть — наведение не гасит кнопку.
    assert_eq!(ui::lighten_hex("transparent", 0.25), "transparent");
}

/// Сторож проводки №2: КАЖДАЯ страница на QML перекрашивается на лету.
///
/// Прошлая починка (3ee6011) научила страницы брать цвета выбранного скина,
/// но свойство `palette` осталось `CONSTANT` — сигнала об изменении не было
/// вовсе, привязки не пересчитывались, и открытая вкладка держала цвета
/// прошлого скина, пока окно на виджетах уже перекрашено. Половина окна в
/// новом скине, половина в старом — покупатель считает это поломкой (#13).
///
/// Проверка общая, по всем `*_page.rs` крейта: механизм рассылки ОДИН
/// (`rexon_gui::skin`), и следующая перенесённая вкладка обязана подписаться
/// теми же двумя строками, а не завести пятую копию рассылки (правило #12).
/// Что перекраска доходит до ЖИВОГО объекта, доказывает замер
/// `scripts/test_qml_skin_live.py` — здесь сторожится только проводка.
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

/// Перевод краски из записи QSS в ту, что понимает QML.
///
/// ⛔ЗАЧЕМ СТОРОЖ. `rgba(...)` — законная запись для QSS виджетов и
/// НЕДОПУСТИМАЯ для QML: страница рисует таким свойством чёрное, не сказав
/// ни слова. Так на вкладке «Steam» вышли чёрными бейджи карточек, крупный
/// чип состояния и звёздочки — дефект жил до попиксельной сверки с
/// оригиналом 01.09.2026.
#[test]
fn css_color_to_qml_переводит_rgba() {
    use rexon_core::ui_base::css_color_to_qml;

    // ⚠️Альфа 0.85 → 0xd8, а НЕ 0xd9: долю разбор таблиц стилей Qt
    // усекает (0.85 * 255 = 216,75 → 216). Пока здесь стояло округление,
    // краска приезжала в QML на ступень гуще виджетной — см. сторож
    // `доля_прозрачности_усекается_как_у_разбора_qss`.
    assert_eq!(css_color_to_qml("rgba(59,130,246,0.85)"), "#d83b82f6");
    assert_eq!(css_color_to_qml("rgba(127,127,127,0.14)"), "#237f7f7f");
    // Полностью непрозрачный — альфа ff.
    assert_eq!(css_color_to_qml("rgba(234,179,8,1.0)"), "#ffeab308");
    // Без альфы.
    assert_eq!(css_color_to_qml("rgb(34,197,94)"), "#ff22c55e");
    // Пробелы в записи законны.
    assert_eq!(css_color_to_qml("rgba( 0, 0, 0, 0.45 )"), "#72000000");
    // Что QML понимает сам — не трогаем.
    for as_is in ["#1a1a1a", "white", "transparent", ""] {
        assert_eq!(css_color_to_qml(as_is), as_is);
    }
    // Мусор возвращается как есть: выдумывать цвет вместо ошибки нельзя.
    assert_eq!(css_color_to_qml("rgba(1,2)"), "rgba(1,2)");
    assert_eq!(css_color_to_qml("rgba(1,2,300,0.5)"), "rgba(1,2,300,0.5)");
}

/// Все краски вкладки «Steam», уходящие в QML, обязаны переводиться.
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
// Поверхности скина для QML: краска приходит ГОТОВОЙ, а не долей
// ============================================================================

/// Разбор доли прозрачности УСЕКАЕТ, как это делает разбор таблиц стилей Qt.
///
/// ⛔ЗАЧЕМ СТОРОЖ. Виджетная часть — эталон, а её краски Qt получает из
/// таблицы стилей, где долю усекает: опыт 04.09 на фоне `#1b2838` (доля
/// против целой альфы «вниз» и «вверх») дал для 0.85 краску альфы 216, для
/// 0.45 — 114, для 0.14 — 35, для 0.25 — 63, для 0.03 — 7, для 0.12 — 30.
/// Пока переводчик округлял, КАЖДАЯ полупрозрачная краска приезжала в QML
/// на ступень гуще виджетной, то есть на тёмном фоне — на ступень светлее.
/// Порог стенда (24 уровня) такое расхождение не показывал: класс ловили
/// поимённо дважды (2916fcc, 5ac5d42), не видя общего корня.
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

/// Накладка, положенная на краску, даёт то же, что рисует Qt на экране.
#[test]
fn накладка_смешивается_как_на_экране() {
    use rexon_core::ui_base::{blend_css_over, CARD_FILL_DARK};

    // Замер 04.09: белая накладка на `#1b2838` даёт у виджетов (36,48,64)
    // при доле 0.04, (54,65,79) при 0.12 и (58,70,83) при 0.14.
    assert_eq!(blend_css_over("#1b2838", CARD_FILL_DARK), "#243040");
    assert_eq!(blend_css_over("#1b2838", "rgba(255,255,255,0.12)"), "#36414f");
    assert_eq!(blend_css_over("#1b2838", "rgba(255,255,255,0.14)"), "#3a4653");
    // Непрозрачная накладка перекрывает основание целиком.
    assert_eq!(blend_css_over("#1b2838", "rgba(102,192,244,1.0)"), "#66c0f4");
    // Разобрать нечего — запись возвращается как есть.
    assert_eq!(blend_css_over("#1b2838", "white"), "white");
}

/// ⛔СТОРОЖ КЛАССА. Любая ПОВЕРХНОСТЬ скина, отдаваемая в QML, обязана
/// приходить ГОТОВОЙ КРАСКОЙ, а не долей и не записью QSS.
///
/// ЗАЧЕМ. Разметка QML не умеет ни ролей палитры Qt, ни каскада таблиц
/// стилей, и страницы выбирали похожий на глаз токен: поле ввода красилось
/// `bg_card` (30,45,61) там, где Qt красит роль `Base` (22,32,45), а
/// карточка — тем же `bg_card` там, где таблица стилей виджетов кладёт
/// накладку 0.04 на фон страницы (36,48,64). Обе краски теперь собирает
/// ядро целиком. Сторож следит, чтобы поверхность не вернулась долей или
/// записью `rgba(...)`: и то и другое QML читает по-своему.
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
        // Палитра страницы обязана нести поверхности рядом с токенами.
        let pal = qml_palette(skin.id);
        for (key, value) in &surfaces {
            assert_eq!(pal.get(key), Some(value), "{key} потерян в палитре QML");
        }
    }
}

/// Поверхность поля — это РОЛЬ `Base` скина, та же, которой Qt красит
/// `QLineEdit` и `QComboBox` у виджетной части.
///
/// Замер 04.09 (опыт на живой Qt): `QLineEdit` с ролью `Base` `#16202d`
/// рисует (22,32,45) — ровно то, что снял стенд с виджетной вкладки
/// «Расширения»; заливка `bg_card` `#1e2d3d` в сцене QML — (30,45,61),
/// ровно то, что стенд снял с QML-вкладки на 37 585 точках поля поиска.
#[test]
fn поверхность_поля_равна_роли_base() {
    use rexon_core::ui_base::{skin_role_color, skin_surface_colors, SKINS};

    for skin in SKINS {
        let Some(base) = skin_role_color(skin.id, "Base") else {
            continue; // системный скин: роли остаются системными
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
    // Кнопка и нередактируемый выпадающий список — другая роль, `Button`.
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

/// Обводка кнопки и поля — накладка стиля Qt, а не роль палитры.
///
/// ⚠️ ЧИСЛА СНЯТЫ С ЖИВОЙ Qt, а не выведены (замер 05.09, скин steam_dark):
/// кнопка «Быстрых пар» «Сравнения» залита (42,64,85) ролью `Button` и
/// обведена (85,102,119); поле поиска «Расширений» залито (22,32,45) ролью
/// `Base`, а обведено (73,83,96) — накладкой поверх ФОНА страницы
/// (27,40,56), куда заливка поля не заходит. Пока разметка QML брала на обе
/// рамки токен `border` (42,64,85), поле расходилось с оригиналом на 31
/// уровень, а у кнопки рамки не было вовсе.
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

    // У каждого скина обводка собрана ТОЙ ЖЕ накладкой: кнопка — поверх
    // своей заливки, поле — поверх фона страницы. Какая из двух накладок
    // (светлая или тёмная) — решает светлота фона, поэтому годится любая.
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

/// Поверхность карточки собрана ТОЙ ЖЕ накладкой, что стоит в таблице
/// стилей виджетной карточки, — иначе две краски разойдутся молча.
#[test]
fn поверхность_карточки_равна_накладке_виджетов() {
    use rexon_core::qt_compare;
    use rexon_core::qt_steam;
    use rexon_core::ui_base::{
        blend_css_over, skin_colors, skin_surface_colors, CARD_FILL_DARK, CARD_FILL_LIGHT,
    };

    // Литерал накладки у витрины Steam (а через неё и у «Расширений») —
    // тот же, что в поверхности.
    assert_eq!(qt_steam::card_colors(false, true).0, CARD_FILL_DARK);
    assert_eq!(qt_steam::card_colors(false, false).0, CARD_FILL_LIGHT);
    // И у «Сравнения»: его таблица стилей карточки набрана тем же.
    assert!(qt_compare::card_qss("deCard", true).contains(CARD_FILL_DARK));
    assert!(qt_compare::card_qss("deCard", false).contains(CARD_FILL_LIGHT));

    // Стандартный скин: накладка 0.04 на `#1b2838` — это (36,48,64), то
    // самое, что стенд снял с виджетной карточки «Сравнения».
    assert_eq!(
        skin_surface_colors("steam_dark").get("surface_card").map(String::as_str),
        Some("#243040")
    );
    // Светлый скин берёт светлую накладку — иначе карточка «светлее белого».
    let light_page = skin_colors("light")["bg_primary"].clone();
    assert_eq!(
        skin_surface_colors("light").get("surface_card").map(String::as_str),
        Some(blend_css_over(&light_page, CARD_FILL_LIGHT).as_str())
    );
}

/// Неактивный текст QML красится ТОЙ ЖЕ краской, что пишет в палитру Qt
/// план скина, — иначе выключенная кнопка на двух версиях гаснет по-разному.
///
/// ЗАЧЕМ. Разметка гасила выключенную кнопку прозрачностью 0.45 на всю
/// кнопку: у виджетов заливка и рамка остаются прежними, а приглушается одна
/// надпись. Замер 05.09 на «Расширениях» (кнопка «Steam Workshop» блока
/// «Установить»): 10 108 точек, (36,48,64) против (31,43,59).
#[test]
fn неактивный_текст_qml_равен_группе_disabled_палитры() {
    use rexon_core::ui_base::{
        disabled_text_color, palette_plan, qml_palette, DISABLED_TEXT_ALPHA, SKINS,
    };

    for skin in SKINS {
        let value = disabled_text_color(skin.id);
        assert_eq!(value.len(), 9, "скин {}: {value} не #aarrggbb", skin.id);

        // Ровно то присваивание, которое исполнитель кладёт в QPalette.
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

        // Палитра страницы обязана её нести: разметке брать больше неоткуда.
        assert_eq!(
            qml_palette(skin.id).get("text_disabled"),
            Some(&value),
            "скин {}: text_disabled потерян в палитре QML",
            skin.id
        );
    }

    // Стандартный скин: белый с прозрачностью 140 — та же ступень, что даёт
    // усечение доли 0.55 в таблицах стилей вкладок.
    assert_eq!(disabled_text_color("steam_dark"), "#8cffffff");
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Сырые ошибки сети на все шесть причин — с именем каталога и без.
const NETWORK_SAMPLES: &[&str] = &[
    "<urlopen error timed out>",
    "Name or service not known",
    "Network Error: Unexpected EOF",
    "Network is unreachable",
    "HTTP Error 503: Service Unavailable",
    "нечто непонятное",
];

/// Скины в «Настройках» по-английски: подписи и описания переведены,
/// признаки (`id`) — те же, что пишет настройка `ui_skin`, так что выбор,
/// сохранённый на русском интерфейсе, узнаётся и на английском.
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

/// Подписи скинов и сетевые ошибки по-русски — дословно как было до
/// перевода (падежи слова «каталог» — целыми фразами).
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
