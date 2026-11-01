//! Golden-сверка порта моделей с оригиналом `rexon/appearance/models.py`.
//!
//! Эталоны сняты с самого Python:
//!   json.dumps(dataclasses.asdict(ThemeItem()), ensure_ascii=False, indent=1)
//!   [ThemeItem(rating=r).rating_stars for r in range(101)]
//! Любое расхождение — ошибка порта, а не теста.

use rexon_core::models::{to_json_indent1, ThemeCategory, ThemeItem};

/// Дефолтный ThemeItem сериализуется байт-в-байт как `asdict` + `indent=1`.
#[test]
fn default_theme_item_serialize_byte_identical() {
    let golden = include_str!("golden/theme_item_default.json");
    let value = serde_json::to_value(ThemeItem::default()).unwrap();
    assert_eq!(to_json_indent1(&value), golden);
}

/// `local_kind` не сериализуется (аналог рантайм-атрибута `_local_kind`,
/// которого нет в `dataclasses.fields`): его наличие не меняет вывод.
#[test]
fn local_kind_is_not_serialized() {
    let golden = include_str!("golden/theme_item_default.json");
    let item = ThemeItem { local_kind: Some("downloaded".to_string()), ..Default::default() };
    let value = serde_json::to_value(&item).unwrap();
    assert_eq!(to_json_indent1(&value), golden);
}

/// Эталон `rating_stars` для rating 0..=100, снятый с Python round.
const RATING_STARS_GOLDEN: &str = "0.0,0.1,0.1,0.1,0.2,0.2,0.3,0.3,0.4,0.5,0.5,0.6,0.6,0.7,0.7,0.8,0.8,0.8,0.9,0.9,1.0,1.1,1.1,1.1,1.2,1.2,1.3,1.4,1.4,1.4,1.5,1.6,1.6,1.6,1.7,1.8,1.8,1.9,1.9,1.9,2.0,2.0,2.1,2.1,2.2,2.2,2.3,2.4,2.4,2.5,2.5,2.5,2.6,2.6,2.7,2.8,2.8,2.9,2.9,3.0,3.0,3.0,3.1,3.1,3.2,3.2,3.3,3.4,3.4,3.5,3.5,3.5,3.6,3.6,3.7,3.8,3.8,3.9,3.9,4.0,4.0,4.0,4.1,4.2,4.2,4.2,4.3,4.3,4.4,4.5,4.5,4.5,4.6,4.7,4.7,4.8,4.8,4.8,4.9,5.0,5.0";

/// `rating_stars` совпадает с Python `round(rating/20.0, 1)` на ВСЕХ 101
/// значениях (включая ties-to-even и float-артефакты вроде rating=1 → 0.1).
#[test]
fn rating_stars_matches_python_round_all_101() {
    let expected: Vec<&str> = RATING_STARS_GOLDEN.split(',').collect();
    for r in 0..=100i64 {
        let item = ThemeItem { rating: r, ..Default::default() };
        // Форматируем так же, как Python repr float с 1 знаком.
        let got = format!("{:.1}", item.rating_stars());
        assert_eq!(got, expected[r as usize], "rating={r}");
    }
}

/// `display_description`: перевод если есть, иначе оригинал.
#[test]
fn display_description_prefers_translation() {
    let only_orig = ThemeItem { description: "orig".into(), ..Default::default() };
    assert_eq!(only_orig.display_description(), "orig");

    let translated = ThemeItem {
        description: "orig".into(),
        description_translated: "перевод".into(),
        ..Default::default()
    };
    assert_eq!(translated.display_description(), "перевод");
}

/// `ThemeCategory::new` заполняет обязательные поля и дефолты dataclass.
#[test]
fn theme_category_new_defaults() {
    let c = ThemeCategory::new(0, "popular", "Популярные", "🔥");
    assert_eq!(c.id, 0);
    assert_eq!(c.key, "popular");
    assert_eq!(c.name, "Популярные");
    assert_eq!(c.icon, "🔥");
    assert_eq!(c.default_sort, "new");
    assert!(!c.virtual_);
    // Каталог по умолчанию — своя категория OCS.
    assert!(!c.local_only());
    assert_eq!(c.ocs_cats(), [0]);
}
