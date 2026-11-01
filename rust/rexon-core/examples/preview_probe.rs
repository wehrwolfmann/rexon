//! Стенд превью плиток «Оформления»: для каждой темы на этой машине —
//! какой источник картинки выбран. Диск только читается.
//!
//!   preview_probe [<home с копией кеша магазина> [<рабочий стол, как в окне: kde>]]
//!   preview_probe --ops <вид> <каталог темы>   — операции картинки одной темы

use std::collections::{BTreeMap, HashMap, HashSet};

use rexon_core::installed_scan::{self as scan, LiveScan};
use rexon_core::local_preview as lp;
use rexon_core::models::ThemeItem;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--ops") {
        let (kind, path) = (std::env::args().nth(2).unwrap_or_default(), std::env::args().nth(3).unwrap_or_default());
        let picture = lp::card_picture(&lp::SampleInput { kind: &kind, install_path: &path, icon_themes: &[] }, "");
        println!("{} ({}): {} операций", picture.source.label(), picture.what, picture.ops.len());
        for op in &picture.ops {
            println!("  {op:?}");
        }
        return;
    }
    let cache_home = std::env::args().nth(1).unwrap_or_default();
    let de = std::env::args().nth(2).unwrap_or_default();
    let (ocs, previews, sections) = if cache_home.is_empty() {
        (Vec::new(), HashMap::new(), Vec::new())
    } else {
        let mut cache = rexon_core::appearance_cache::LiveCache::open(&cache_home);
        (
            cache.all_items_min(),
            cache.all_previews_min().into_iter().collect(),
            cache.all_preview_categories(),
        )
    };
    let live = LiveScan::new(ocs, previews).with_store_records(sections).with_de(&de).read_only();
    let active = live.active_map();
    let mut sections: Vec<(String, Vec<ThemeItem>)> = vec![
        ("active".into(), live.active(&active)),
        ("downloaded".into(), live.downloaded(&active)),
        ("system".into(), live.system(&active).unwrap_or_default()),
        ("kvantum".into(), live.kvantum(&active)),
        ("splash".into(), live.lnf("splash", &active)),
        ("lockscreen".into(), live.lnf("lockscreen", &active)),
    ];
    let styles: Vec<String> = scan::WIDGET_STYLES.iter().map(|s| s.to_string()).collect();
    sections.push(("widgetstyle".into(), live.widget_styles(&active, &styles, true)));
    for (_, key) in rexon_core::categories::KDE_INSTALLED_SECTIONS
        .iter()
        .chain(rexon_core::categories::DE_SECTIONS)
    {
        sections.push((key.to_string(), live.section(key, &active)));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let icon_themes = lp::icon_theme_dirs(&home, "/usr");
    let mut seen: HashSet<String> = HashSet::new();
    let mut by_kind: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for (section, items) in &sections {
        println!("== {section}: {}", items.len());
        for it in items {
            let kind = rexon_core::de_apply::apply_kind_of_local_id(&it.id).unwrap_or("?");
            let (source, detail) = if it.preview_local.is_empty() {
                let picture = lp::card_picture(
                    &lp::SampleInput { kind, install_path: &it.install_path, icon_themes: &icon_themes },
                    &lp::type_emoji(&it.author),
                );
                // У рамок MATE причина заглушки называется словами.
                let detail = match (kind, picture.source) {
                    ("marco", lp::PreviewSource::Placeholder) => lp::marco_failure(&it.install_path).to_string(),
                    _ => picture.what.to_string(),
                };
                (picture.source, detail)
            } else {
                (lp::file_source(&it.preview_local, &it.install_path), it.preview_local.clone())
            };
            println!("  {:<10} {:<44} {:<10} {}", kind, it.name, source.label(), detail);
            if seen.insert(it.id.clone()) {
                *by_kind.entry(kind.to_string()).or_default().entry(source.label()).or_default() += 1;
            }
        }
    }
    println!("== СВОДКА по видам (каждая тема один раз)");
    let mut total: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (kind, counts) in &by_kind {
        println!("  {kind:<18} {counts:?}");
        for (k, v) in counts {
            *total.entry(k).or_default() += v;
        }
    }
    println!("  ИТОГО              {total:?}");
}
