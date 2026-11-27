//! AUR package card: parsing the RPC `info` record, the package page
//! (co-maintainers, comments) and picking the picture.
//!
//! Golden files are RECORDED live responses from aur.archlinux.org of 05.10.2026:
//! `aur_info_detail.json` (RPC v5 `info` for four packages) and
//! `aur_page_<name>.html` (whole package pages). The network takes no part
//! in the checks.

use rexon_core::aur::{
    detail_image_sources, epoch_day, info_facts, opt_depend_line, package_page_url,
    page_pause_left_ms, parse_info, parse_og_image, parse_package_page, readme_first_image, AurInfo,
    PageError, OG_IMAGE_SCHEME, PAGE_PAUSE_MS,
};
use serde_json::Value;

fn info_json() -> Value {
    serde_json::from_str(include_str!("golden/aur_info_detail.json")).unwrap()
}

fn info(name: &str) -> AurInfo {
    parse_info(&info_json(), name).unwrap_or_else(|| panic!("нет {name} в эталоне"))
}

#[test]
fn info_aerothemeplasma_all_fields() {
    let i = info("aerothemeplasma-desktop-git");
    assert_eq!(i.version, "6.7.4_735.rc9afeb7-1");
    assert_eq!(i.maintainer, "WackyIdeas");
    assert_eq!(i.submitter, "WackyIdeas");
    assert_eq!(i.votes, 3);
    assert_eq!(i.first_submitted, 1772325181);
    assert_eq!(i.last_modified, 1787168871);
    assert_eq!(i.out_of_date, 0);
    assert_eq!(i.license, vec!["AGPL-3.0-or-later"]);
    assert_eq!(i.url, "https://github.com/aeroshell-desktop/aerothemeplasma");
    assert_eq!(i.depends.len(), 55);
    assert_eq!(i.depends[0], "gcc-libs");
    assert_eq!(i.make_depends, vec!["git", "extra-cmake-modules", "plasma-wayland-protocols"]);
    assert_eq!(i.opt_depends, vec!["uac-polkit-agent", "qrca"]);
    assert_eq!(i.conflicts, vec!["aerothemeplasma-desktop"]);
    assert_eq!(i.provides, vec!["aerothemeplasma-desktop"]);
    assert!(i.replaces.is_empty());
    assert!(i.keywords.is_empty());
    assert_eq!(i.groups, vec!["aerothemeplasma"]);
}

#[test]
fn info_zuki_and_bibata() {
    let z = info("zuki-themes");
    assert_eq!(z.version, "4.0-1");
    assert_eq!(z.votes, 483);
    assert_eq!(z.keywords, vec!["GTK", "theme"]);
    assert_eq!(z.license, vec!["GPL3"]);
    assert_eq!(z.opt_depends, vec!["ttf-roboto"]);
    let b = info("bibata-cursor-theme");
    assert_eq!(b.make_depends, vec!["python-clickgen>=2.0.0"]);
    assert!(b.depends.is_empty());
    assert_eq!(b.maintainer, "yochananmarqos");
    // A tile from the record — for the common show/install path.
    let p = b.as_package();
    assert_eq!(p.name, "bibata-cursor-theme");
    assert_eq!(p.upstream_url, "https://github.com/ful1e5/Bibata_Cursor");
    assert!(!p.out_of_date);
}

#[test]
fn info_unknown_name_is_none_and_garbage_tolerated() {
    assert!(parse_info(&info_json(), "no-such-package").is_none());
    assert!(parse_info(&serde_json::json!({"results": null}), "x").is_none());
    // Numbers as strings and junk in arrays do not break the parse.
    let v = serde_json::json!({"results": [{
        "Name": "x", "NumVotes": "7", "OutOfDate": "1700000000",
        "Depends": ["a", 5, null, "", "b"]
    }]});
    let i = parse_info(&v, "x").unwrap();
    assert_eq!(i.votes, 7);
    assert_eq!(i.out_of_date, 1700000000);
    assert_eq!(i.depends, vec!["a", "b"]);
}

#[test]
fn facts_order_and_out_of_date_warning() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let mut i = info("zuki-themes");
    let facts = info_facts(&i, &["Antiz".to_string()]);
    let labels: Vec<&str> = facts.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels[0], "Version");
    assert!(labels.contains(&"Co-maintainers"));
    assert!(labels.contains(&"Keywords"));
    assert!(!labels.contains(&"Replaces"), "пустые поля не показываются");
    let page = facts.iter().find(|f| f.label == "Project page").unwrap();
    assert!(page.link);
    assert_eq!(facts.iter().find(|f| f.label == "First submitted").unwrap().value, "2016-05-22");
    assert!(facts.iter().all(|f| !f.warn));

    i.out_of_date = 1700000000;
    let facts = info_facts(&i, &[]);
    assert_eq!(facts[0].label, "Status");
    assert!(facts[0].warn);
    assert!(facts[0].value.contains("2023-11-14"));
}

#[test]
fn opt_depends_readable() {
    assert_eq!(opt_depend_line("ttf-roboto"), "ttf-roboto");
    assert_eq!(opt_depend_line("ffmpeg: play videos"), "ffmpeg — play videos");
    assert_eq!(opt_depend_line("zenity:"), "zenity");
    assert_eq!(epoch_day(0), "");
    assert_eq!(epoch_day(1787168871), "2026-08-19");
}

#[test]
fn page_bibata_comments_with_edits_and_next() {
    let p = parse_package_page(include_str!("golden/aur_page_bibata-cursor-theme.html")).unwrap();
    assert!(p.comaintainers.is_empty());
    assert_eq!(p.comments.len(), 10);
    assert!(p.has_more);
    let first = &p.comments[0];
    assert_eq!(first.author, "deathtrip");
    assert_eq!(first.date, "2023-09-30 14:14 (UTC)");
    assert_eq!(first.text, "I get an error: cp: cannot stat 'themes/Bibata-*': No such file or directory");
    assert!(!first.pinned);
    // The edit mark is not confused with the date and the nickname.
    let edited = p.comments.iter().find(|c| c.author == "crabvk").unwrap();
    assert_eq!(edited.date, "2022-10-06 21:57 (UTC)");
    assert!(p.comments.iter().all(|c| !c.text.contains('<') && !c.author.is_empty()));
}

#[test]
fn page_yay_pinned_first_and_code_blocks_kept() {
    let p = parse_package_page(include_str!("golden/aur_page_yay.html")).unwrap();
    let pinned: Vec<_> = p.comments.iter().filter(|c| c.pinned).collect();
    assert_eq!(pinned.len(), 2);
    assert!(p.comments[0].pinned && p.comments[1].pinned, "закреплённые — первыми");
    // The site has 2 pinned and 9 latest: deleted comments are not shown
    // to a guest, so a page can be incomplete — the "there is more" flag
    // is taken from the «Next» link, not from the count.
    assert_eq!(p.comments.len(), 11);
    assert!(p.has_more);
    let c = &p.comments[0];
    assert_eq!(c.author, "jguer");
    assert_eq!(c.date, "2025-12-14 20:11 (UTC)");
    // Paragraphs as separate lines, code with its own line breaks.
    assert!(c.text.starts_with("yay: error while loading shared libraries"));
    assert!(c.text.contains("\n\nall v12.5.7 binaries"));
    assert!(c.text.contains("git clone https://aur.archlinux.org/yay.git\ncd yay\nmakepkg -si"));
}

#[test]
fn page_spotify_comaintainers() {
    let p = parse_package_page(include_str!("golden/aur_page_spotify.html")).unwrap();
    assert_eq!(p.comaintainers, vec!["Antiz"]);
    assert_eq!(p.comments.iter().filter(|c| c.pinned).count(), 2);
    assert_eq!(p.comments.len(), 12);
}

#[test]
fn page_zuki_short_page_with_next_and_aerothemeplasma_single() {
    // 8 comments on the first page out of 13: an incomplete page is not the end.
    let z = parse_package_page(include_str!("golden/aur_page_zuki-themes.html")).unwrap();
    assert_eq!(z.comments.len(), 8);
    assert!(z.has_more);
    let a = parse_package_page(include_str!("golden/aur_page_aerothemeplasma-desktop-git.html")).unwrap();
    assert_eq!(a.comments.len(), 1);
    assert_eq!(a.comments[0].author, "2Epik4u");
    assert_eq!(a.comments[0].date, "2026-06-19 18:43 (UTC)");
    assert!(a.comments[0].text.starts_with("Building a lot of the packages in aerothemeplasma gets me"));
    assert!(!a.has_more);
}

#[test]
fn bot_check_page_is_blocked_not_empty() {
    // A synthetic challenge page (a real Anubis one cannot be captured as a golden file:
    // on 05.10 the site served pages without it). The point is that this is NOT "0 comments".
    let html = "<html><head><title>Making sure you're not a bot!</title></head>\
                <body><div id=\"anubis_challenge\"></div></body></html>";
    assert_eq!(parse_package_page(html), Err(PageError::Blocked));
    assert_eq!(parse_package_page(""), Err(PageError::Blocked));
}

#[test]
fn page_urls_and_pause() {
    assert_eq!(package_page_url("zuki-themes", 0), "https://aur.archlinux.org/packages/zuki-themes");
    assert_eq!(package_page_url("yay", 2), "https://aur.archlinux.org/packages/yay?O=20");
    assert_eq!(page_pause_left_ms(None), 0);
    assert_eq!(page_pause_left_ms(Some(500)), PAGE_PAUSE_MS - 500);
    assert_eq!(page_pause_left_ms(Some(PAGE_PAUSE_MS + 1)), 0);
}

#[test]
fn image_sources_by_host() {
    let gh = detail_image_sources("https://github.com/aeroshell-desktop/aerothemeplasma");
    // There is no OpenGraph repository card (name and stars): it is not a screenshot.
    assert_eq!(gh, vec!["github-readme:aeroshell-desktop/aerothemeplasma".to_string()]);
    // The card's first picture is the same as the tile's.
    let tile = info("aerothemeplasma-desktop-git").as_package().preview_url();
    assert_eq!(gh[0], tile);
    let cb = detail_image_sources("https://codeberg.org/forgejo/forgejo");
    assert_eq!(cb, vec![format!("{OG_IMAGE_SCHEME}https://codeberg.org/forgejo/forgejo")]);
    assert_eq!(detail_image_sources("https://gitlab.gnome.org/GNOME/x").len(), 1);
    assert_eq!(detail_image_sources("https://invent.kde.org/plasma/breeze").len(), 1);
    assert!(detail_image_sources("https://www.example.org/").is_empty());
    assert!(detail_image_sources("").is_empty());
}

#[test]
fn og_image_codeberg_and_gitlab() {
    assert_eq!(
        parse_og_image(include_str!("golden/og_codeberg_head.html"), "https://codeberg.org/forgejo/forgejo")
            .as_deref(),
        Some("https://codeberg.org/forgejo/forgejo/-/summary-card")
    );
    // GitLab has the attributes in a different order: content before property.
    assert_eq!(
        parse_og_image(include_str!("golden/og_gitlab_head.html"), "https://gitlab.com/inkscape/inkscape")
            .as_deref(),
        Some("https://gitlab.com/uploads/-/system/project/avatar/3472737/inkscape.png?v=1791140477")
    );
    assert_eq!(
        parse_og_image("<meta property=\"og:image\" content=\"/a.png\">", "https://h.org/x/y").as_deref(),
        Some("https://h.org/a.png")
    );
    assert!(parse_og_image("<meta property=\"og:title\" content=\"x\">", "https://h.org").is_none());
}

/// A picture from the README (the second picture of a GitHub package card): badges
/// are skipped, relative links and `blob/` lead to the branch's "raw" view.
#[test]
fn readme_image_skips_badges_and_resolves_relative() {
    let md = "# T\n[![CI](https://img.shields.io/x.svg)](x)\n![preview](./assets/preview.png)\n";
    assert_eq!(
        readme_first_image(md, "a/b", "main").as_deref(),
        Some("https://raw.githubusercontent.com/a/b/main/assets/preview.png")
    );
    let html = "<p align=center><img src=\"https://github.com/a/b/blob/main/p.gif?raw=true\" width=300></p>";
    assert_eq!(
        readme_first_image(html, "a/b", "main").as_deref(),
        Some("https://raw.githubusercontent.com/a/b/main/p.gif")
    );
    assert_eq!(readme_first_image("no pictures", "a/b", "main"), None);
    // SVG — only if there is nothing else.
    let both = "![](logo.svg)\n![](shot.png)";
    assert!(readme_first_image(both, "a/b", "m").unwrap().ends_with("shot.png"));
}
