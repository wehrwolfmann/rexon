//! Сверка разбора характерных цветов схемы KDE и знака вида
//! (`rexon_core::local_preview`) с записанным эталоном.
//!
//! `golden/local_preview_cases.json` — снятые значения: `parse_colors` —
//! краевые случаи на подставных текстах, `parse_colors_real` — все настоящие
//! схемы `/usr/share/color-schemes` машины, где эталон снимался (51 файл,
//! тексты лежат в эталоне), `type_emoji` — знак вида из подписи плитки.
//!
//! Случай `out_of_range`: компоненты вне 0..=255 отбрасываются целиком (в
//! эталоне на их месте записан чёрный цвет — так рисовала прежняя отрисовка).

use rexon_core::local_preview as lp;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/local_preview_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn rgb(v: &Value) -> (u8, u8, u8) {
    let a = v.as_array().unwrap();
    (
        a[0].as_u64().unwrap() as u8,
        a[1].as_u64().unwrap() as u8,
        a[2].as_u64().unwrap() as u8,
    )
}

#[test]
fn scheme_colors_parse() {
    let g = fx();
    for case in g["parse_colors"].as_array().unwrap() {
        let label = s(&case["label"]);
        // Несуществующий файл ядру достаётся пустым текстом.
        let text = case["text"].as_str().unwrap_or("");
        let expected: Vec<(u8, u8, u8)> = if label == "out_of_range" {
            // В эталоне на месте записи `-1,300,12` лежит второй чёрный
            // цвет; запись отбрасывается целиком — остаётся один настоящий
            // (0,0,0) из `DecorationFocus=0,0,0`.
            vec![(0, 0, 0)]
        } else {
            case["colors"].as_array().unwrap().iter().map(rgb).collect()
        };
        assert_eq!(
            lp::parse_scheme_colors(text),
            expected,
            "parse_colors {label}"
        );
    }
}

#[test]
fn emoji_from_author() {
    let g = fx();
    for case in g["type_emoji"].as_array().unwrap() {
        let author = s(&case["author"]);
        assert_eq!(
            lp::type_emoji(author),
            s(&case["emoji"]),
            "type_emoji {author:?}"
        );
    }
}

/// Разбор цветов на настоящих схемах (Breeze*, CachyOSNord*, Kv*, Emerald* …):
/// подставные случаи выше проверяют края, а этот — что на схемах KDE берутся
/// те же цвета в том же порядке (порядок задаёт полосы образца).
#[test]
fn scheme_colors_parse_real_installed() {
    let g = fx();
    let cases = g["parse_colors_real"].as_array().unwrap();
    assert!(cases.len() >= 30, "мало реальных схем в эталоне: {}", cases.len());
    let mut six = 0;
    for case in cases {
        let file = s(&case["file"]);
        let want: Vec<(u8, u8, u8)> = case["colors"].as_array().unwrap().iter().map(rgb).collect();
        let got = lp::parse_scheme_colors(s(&case["text"]));
        assert_eq!(got, want, "цвета схемы {file}");
        assert!(
            got.len() <= lp::MAX_SCHEME_COLORS,
            "{file}: больше {} цветов",
            lp::MAX_SCHEME_COLORS
        );
        if got.len() == lp::MAX_SCHEME_COLORS {
            six += 1;
        }
        // Настоящая схема обязана дать хотя бы один цвет — иначе плитка
        // молча осталась бы с заглушкой вида.
        assert!(!got.is_empty(), "{file}: ни одного цвета");
    }
    assert!(six >= 20, "мало схем с полным набором из 6 цветов: {six}");
}

/// Мелкие миниатюры тем (B00merang: 120×35 и 100×32) не растягиваются на
/// плитку: кадр собирается из них, каждая вписана и не крупнее двух своих
/// размеров.
#[test]
fn small_theme_thumbnails_are_composed_not_stretched() {
    use rexon_core::local_preview as lp;
    let root = std::env::temp_dir().join(format!("rexon-thumbs-{}", std::process::id()));
    let png = |rel: &str, w: u32, h: u32| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        // Заголовок PNG с IHDR — image_size читает только его.
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        std::fs::write(&p, b).unwrap();
        p.to_string_lossy().into_owned()
    };
    let gtk = png("gtk-3.0/thumbnail.png", 120, 35);
    let frame = png("metacity-1/thumbnail.png", 100, 32);
    assert!(lp::is_small_thumbnail(&gtk) && lp::is_small_thumbnail(&frame));
    let big = png("big/thumbnail.png", 600, 340);
    assert!(!lp::is_small_thumbnail(&big));
    let mut items = vec![rexon_core::models::ThemeItem {
        id: "local:gtk:XP".into(),
        preview_local: gtk.clone(),
        ..Default::default()
    }];
    lp::drop_small_thumbnails(&mut items);
    assert!(items[0].preview_local.is_empty());
    let path = root.to_string_lossy().into_owned();
    let pic = lp::card_picture(&lp::SampleInput { kind: "gtk", install_path: &path, icon_themes: &[] }, "🎨");
    assert_eq!(pic.source, lp::PreviewSource::Sample);
    let images: Vec<_> = pic
        .ops
        .iter()
        .filter_map(|op| match op {
            lp::DrawOp::Image { rect, path, cover } => Some((*rect, path.clone(), *cover)),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2, "рамка и элементы");
    assert_eq!(images[0].1, frame, "рамка окна сверху");
    for ((x, y, w, h), _, cover) in &images {
        assert!(!cover, "вписать, а не обрезать");
        assert!(*w <= 240.0 + 0.5 && *h <= 70.0 + 0.5, "не крупнее двух размеров: {w}×{h}");
        assert!(*x >= 0.0 && *y >= 0.0 && x + w <= 300.0 && y + h <= 170.0);
    }
    let _ = std::fs::remove_dir_all(&root);
}
