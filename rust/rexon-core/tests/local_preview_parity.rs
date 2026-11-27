//! Check of parsing the characteristic colors of a KDE scheme and the kind sign
//! (`rexon_core::local_preview`) against the recorded golden.
//!
//! `golden/local_preview_cases.json` — captured values: `parse_colors` —
//! edge cases on fake texts, `parse_colors_real` — all real
//! schemes in `/usr/share/color-schemes` of the machine where the golden was captured (51 files,
//! the texts lie in the golden), `type_emoji` — the kind sign from the tile label.
//!
//! The `out_of_range` case: components outside 0..=255 are dropped entirely (in
//! the golden a black color is recorded in their place — that is how the former rendering drew it).

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
        // A nonexistent file reaches the core as an empty text.
        let text = case["text"].as_str().unwrap_or("");
        let expected: Vec<(u8, u8, u8)> = if label == "out_of_range" {
            // In the golden, in place of the entry `-1,300,12` there is a second black
            // color; the entry is dropped entirely — one real one remains
            // (0,0,0) from `DecorationFocus=0,0,0`.
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

/// Color parsing on real schemes (Breeze*, CachyOSNord*, Kv*, Emerald* …):
/// the fake cases above check the edges, and this one checks that the same colors are taken from KDE schemes
/// in the same order (the order sets the sample stripes).
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
        // A real scheme must give at least one color — otherwise the tile
        // would silently stay with the kind placeholder.
        assert!(!got.is_empty(), "{file}: ни одного цвета");
    }
    assert!(six >= 20, "мало схем с полным набором из 6 цветов: {six}");
}

/// Small theme thumbnails (B00merang: 120×35 and 100×32) are not stretched over
/// the tile: the frame is assembled from them, each one fitted and no larger than twice its
/// size.
#[test]
fn small_theme_thumbnails_are_composed_not_stretched() {
    use rexon_core::local_preview as lp;
    let root = std::env::temp_dir().join(format!("rexon-thumbs-{}", std::process::id()));
    let png = |rel: &str, w: u32, h: u32| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        // A PNG header with IHDR — image_size reads only that.
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
