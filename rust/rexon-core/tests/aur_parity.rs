//! Golden check of the AUR parser against the original `rexon/appearance/aur_client.py`.
//!
//! The golden files were captured from Python itself (see the generator): `aur_search_raw.json` is
//! a REAL RPC response (`curl` search/aerothemeplasma), `aur_cases.json` is
//! the oracle's outputs (the real `_parse`/attributes/`search_aur`/`fetch_for_category`
//! with the network monkeypatched). Rust must return the same models/strings/order.
//!
//! ⚠️ The live network call is NOT tested (non-deterministic and not ported
//! to the core) — only the PARSING of a recorded response is checked.

use rexon_core::aur::{
    check_rpc_error, curated_order, dedup_category, parse_results, search_filter, AurPackage,
};
use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/aur_cases.json")).unwrap()
}

/// Parsing a REAL RPC response: all fields and computed attributes match
/// Python `_parse` + the `AurPackage` properties.
#[test]
fn parse_real_response_matches_python() {
    let raw: Value = serde_json::from_str(include_str!("golden/aur_search_raw.json")).unwrap();
    let got = parse_results(&raw);
    let want = cases()["parsed_real"].as_array().unwrap().clone();
    assert_eq!(got.len(), want.len(), "число пакетов");
    assert!(!got.is_empty(), "фикстура не должна быть пустой");
    for (p, w) in got.iter().zip(&want) {
        assert_eq!(p.name, w["name"].as_str().unwrap());
        assert_eq!(p.description, w["description"].as_str().unwrap());
        assert_eq!(p.version, w["version"].as_str().unwrap());
        assert_eq!(p.votes, w["votes"].as_i64().unwrap());
        assert_eq!(p.popularity, w["popularity"].as_f64().unwrap());
        assert_eq!(p.maintainer, w["maintainer"].as_str().unwrap());
        assert_eq!(p.out_of_date, w["out_of_date"].as_bool().unwrap());
        assert_eq!(p.upstream_url, w["upstream_url"].as_str().unwrap());
        assert_eq!(p.last_modified, w["last_modified"].as_i64().unwrap());
        // computed
        assert_eq!(p.id(), w["id"].as_str().unwrap());
        assert_eq!(p.aur_url(), w["aur_url"].as_str().unwrap());
        assert_eq!(p.preview_url(), readme_marker(w["preview_url"].as_str().unwrap()));
        assert_eq!(p.icon(), w["icon"].as_str().unwrap());
    }
}

/// The Python golden file gave the repository's OpenGraph card; since 16.11.2026 the tile
/// takes the first README picture of the same repository (a card with the name and
/// stars is not a product screenshot). The URL parsing is the same, so the golden file
/// is checked through a replacement of the URL form.
fn readme_marker(og: &str) -> String {
    og.strip_prefix("https://opengraph.githubassets.com/0/")
        .map(|rest| format!("{}{rest}", rexon_core::aur::README_PREVIEW_SCHEME))
        .unwrap_or_else(|| og.to_string())
}

/// preview_url: the github regexp (owner/repo, .git and the tail are cut off),
/// non-github → "".
#[test]
fn preview_url_matches_python() {
    for (url, want) in cases()["url_cases"].as_object().unwrap() {
        let p = AurPackage { upstream_url: url.clone(), ..Default::default() };
        assert_eq!(p.preview_url(), readme_marker(want.as_str().unwrap()), "preview для {url}");
        assert!(!p.preview_url().contains("opengraph"), "OG-карточка на плитке: {url}");
    }
}

/// A font tile is a Google Fonts sample by package name; a wallpaper set and
/// others without a screenshot get an honest caption (16.11.2026).
#[test]
fn font_and_wallpaper_tiles_show_the_goods_or_say_so() {
    use rexon_core::aur::*;
    assert_eq!(google_font_family("ttf-fira-sans").as_deref(), Some("Fira Sans"));
    assert_eq!(google_font_family("otf-source-code-pro-git").as_deref(), Some("Source Code Pro"));
    assert_eq!(google_font_family("ttf-roboto-variable").as_deref(), Some("Roboto"));
    assert_eq!(google_font_family("ttf-git"), None);
    assert_eq!(google_font_family("plasma6-wallpapers-x"), None);
    assert_eq!(google_font_css_url("Fira Sans"), "https://fonts.googleapis.com/css2?family=Fira+Sans");
    let css = "@font-face {\n  font-family: 'Fira Sans';\n  src: url(https://fonts.gstatic.com/s/firasans/v18/x.ttf) format('truetype');\n}";
    assert_eq!(parse_google_font_css(css).as_deref(), Some("https://fonts.gstatic.com/s/firasans/v18/x.ttf"));
    assert_eq!(parse_google_font_css("<html>400</html>"), None);

    let font = AurPackage {
        name: "ttf-fira-sans".into(),
        upstream_url: "https://github.com/mozilla/Fira".into(),
        category: "fonts".into(),
        ..Default::default()
    };
    assert_eq!(font.preview_url(), format!("{GOOGLE_FONT_SCHEME}Fira Sans"), "у шрифта — образец, не README");
    assert!(!no_preview_note(&font).is_empty());

    let walls = AurPackage {
        name: "archlinux-wallpaper".into(),
        upstream_url: "https://example.org/".into(),
        category: "wallpapers".into(),
        ..Default::default()
    };
    assert_eq!(walls.preview_url(), "", "картинки взять неоткуда");
    assert!(!no_preview_note(&walls).is_empty(), "набор обоев без снимка — честная подпись");
    let theme = AurPackage { name: "x-icon-theme".into(), category: "icons".into(), ..Default::default() };
    assert!(!no_preview_note(&theme).is_empty());
    let shown = AurPackage { upstream_url: "https://github.com/a/b".into(), category: "icons".into(), ..theme };
    assert_eq!(no_preview_note(&shown), "", "снимок README будет — подпись не нужна");
}

/// AUR search inside a section: only the section's goods. Live check
/// 16.11.2026 — «girl» in «Обои HD» brought fcitx dictionaries, a font and
/// a game engine.
#[test]
fn search_inside_a_section_keeps_only_its_kind() {
    use rexon_core::aur::*;
    let pkg = |name: &str, desc: &str| AurPackage { name: name.into(), description: desc.into(), ..Default::default() };
    let found = vec![
        pkg("rime-pinyin-moegirl", "Moegirl dictionary for rime"),
        pkg("fcitx5-pinyin-moegirl", "Fcitx 5 pinyin dictionary from zh.moegirl.org.cn"),
        pkg("ttf-league-script-number-one", "A modern coquettish script font"),
        pkg("catgirl", "TLS-only terminal IRC client"),
        pkg("violetland-git", "Help a girl by name of Violet to struggle with hordes of monsters"),
        pkg("anime-girl-wallpapers", "Anime girl wallpapers"),
        pkg("girl-backgrounds", "Desktop backgrounds collection"),
        pkg("girl-wallpaper-changer", "Changes your wallpaper with girls"),
    ];
    let walls: Vec<String> = filter_search_for_category(found.clone(), "wallpapers").into_iter().map(|p| p.name).collect();
    assert_eq!(walls, ["anime-girl-wallpapers", "girl-backgrounds"]);
    assert!(filter_search_for_category(found.clone(), "wallpapers").iter().all(|p| p.category == "wallpapers"));
    let fonts: Vec<String> = filter_search_for_category(found.clone(), "fonts").into_iter().map(|p| p.name).collect();
    assert_eq!(fonts, ["ttf-league-script-number-one"]);
    // A section without an AUR shelf — results as is.
    assert_eq!(filter_search_for_category(found.clone(), "нет-такого").len(), found.len());
}

/// icon: by category, otherwise a name heuristic (check order as in Python).
#[test]
fn icon_matches_python() {
    for case in cases()["icon_cases"].as_array().unwrap() {
        let p = AurPackage {
            name: case["name"].as_str().unwrap().to_string(),
            category: case["category"].as_str().unwrap().to_string(),
            ..Default::default()
        };
        assert_eq!(p.icon(), case["icon"].as_str().unwrap(), "icon для {case:?}");
    }
}

/// aur_url: percent-encode the name like `urllib.parse.quote`.
#[test]
fn aur_url_matches_python() {
    for (name, want) in cases()["aururl_cases"].as_object().unwrap() {
        let p = AurPackage { name: name.clone(), ..Default::default() };
        assert_eq!(&p.aur_url(), want.as_str().unwrap(), "aur_url для {name}");
    }
}

/// search_aur: local AND filter of a multi-word query + sort_votes.
#[test]
fn search_filter_matches_python() {
    let sc = &cases()["search"];
    let resp = sc["response"].clone();
    let query = sc["query"].as_str().unwrap();
    let got: Vec<String> = search_filter(&resp, query, None)
        .into_iter()
        .map(|p| p.name)
        .collect();
    let want: Vec<String> = sc["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want);
}

/// fetch_for_category("global"): dedup by name (the first wins),
/// sort_votes, aerothemeplasma pinned to the top; category is set.
/// `aerothemeplasma-icons-git` is NOT in «Глобальные темы» since 03.10.2026 — it
/// lives in «Значки» (`aur::is_relevant`).
#[test]
fn dedup_category_matches_python() {
    let dc = &cases()["dedup_global"];
    let responses: Vec<Value> = dc["responses"].as_array().unwrap().clone();
    let refs: Vec<&Value> = responses.iter().collect();
    let key = dc["key"].as_str().unwrap();
    let out = dedup_category(&refs, key);

    let got: Vec<String> = out.iter().map(|p| p.name.clone()).collect();
    let want: Vec<String> = dc["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want, "порядок дедупа/пина");
    // category of each = "global"
    for p in &out {
        assert_eq!(p.category, key);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Closing parity holes: the curated list, "a field of the wrong type", RPC errors
//  and malformed/truncated responses. The golden values were captured from the live aur_client.py
//  on the Python path before its removal (the generator is ad hoc, the input is given in each test).
// ═══════════════════════════════════════════════════════════════════════════

/// `fetch_curated` on a REAL AUR response: the `aerothemeplasma*` pin goes in
/// `CURATED` order.
///
/// ⚠️ The test pins a QUIRK of the original: `aerothemeplasma-*` packages that are not
/// in `CURATED` (`-desktop-x11-git` in the live response) drop out of the results entirely
/// — they do not get into the pin (not in the list), and they are filtered out of "the rest" by
/// prefix. The behaviour is kept deliberately: the selection shows exactly what
/// is in `CURATED`. `-sounds-git` is in `CURATED` since 03.10.2026 and pinned third.
#[test]
fn curated_order_on_real_response() {
    let raw: Value = serde_json::from_str(include_str!("golden/aur_search_raw.json")).unwrap();
    let got: Vec<String> = curated_order(&raw).into_iter().map(|p| p.name).collect();
    let want: Vec<String> = cases()["curated_real"]["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want);
    assert!(!want.is_empty(), "фикстура не должна быть пустой");
}

/// `fetch_curated` on a mixed response: pinned in `CURATED` order, the rest by
/// votes (descending), an entry without `Name` skipped (the original crashed with KeyError).
#[test]
fn curated_order_mixed_and_nameless_record() {
    let c = &cases()["curated_mixed"];
    let got: Vec<String> = curated_order(&c["response"])
        .into_iter()
        .map(|p| p.name)
        .collect();
    let want: Vec<String> = c["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want);
}

/// "A field of the wrong type": numbers as strings/fractions/bool, junk and null give exactly
/// the same values as the fixed Python (`_num`), without a panic.
#[test]
fn loose_typed_fields_match_python() {
    let c = &cases()["loose_types"];
    let got = parse_results(&c["response"]);
    let want = c["expected"].as_array().unwrap();
    assert_eq!(got.len(), want.len());
    for (p, w) in got.iter().zip(want) {
        assert_eq!(p.name, w["name"].as_str().unwrap());
        assert_eq!(p.votes, w["votes"].as_i64().unwrap(), "votes {}", p.name);
        assert_eq!(
            p.popularity,
            w["popularity"].as_f64().unwrap(),
            "popularity {}",
            p.name
        );
        assert_eq!(
            p.last_modified,
            w["last_modified"].as_i64().unwrap(),
            "last_modified {}",
            p.name
        );
        assert_eq!(
            p.out_of_date,
            w["out_of_date"].as_bool().unwrap(),
            "out_of_date {}",
            p.name
        );
    }
}

/// A response with an RPC error (including the real `type=error` format) is recognized with the
/// same text as in Python; a normal response is not considered an error.
#[test]
fn rpc_error_detection_matches_python() {
    for c in cases()["rpc_errors"].as_array().unwrap() {
        let got = check_rpc_error(&c["response"]);
        match c["error"].as_str() {
            Some(msg) => assert_eq!(got, Err(msg.to_string()), "для {:?}", c["response"]),
            None => assert_eq!(got, Ok(()), "для {:?}", c["response"]),
        }
    }
}

/// Malformed and truncated responses: `{}`, `results: null`, a string instead of an array,
/// non-objects inside the array. The parse must return the same as the fixed
/// Python, and in NO case panic.
#[test]
fn broken_responses_degrade_like_python() {
    for c in cases()["broken_responses"].as_array().unwrap() {
        let data = &c["response"];

        let parsed: Vec<String> = parse_results(data).into_iter().map(|p| p.name).collect();
        let want_parsed: Vec<String> = c["parsed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(parsed, want_parsed, "parse_results для {data:?}");

        let curated: Vec<String> = curated_order(data).into_iter().map(|p| p.name).collect();
        let want_curated: Vec<String> = c["curated"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(curated, want_curated, "curated_order для {data:?}");

        // search_filter on a single word — the same path as _py_search_aur.
        let search: Vec<String> = search_filter(data, "survivor", None)
            .into_iter()
            .map(|p| p.name)
            .collect();
        let want_search: Vec<String> = c["search"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(search, want_search, "search_filter для {data:?}");

        let dedup: Vec<String> = dedup_category(&[data], "kvantum")
            .into_iter()
            .map(|p| p.name)
            .collect();
        let want_dedup: Vec<String> = c["dedup"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(dedup, want_dedup, "dedup_category для {data:?}");
    }
}

/// The response body is HTML (an error/captcha page instead of JSON). Such a thing does not
/// reach the core: `_http_json` fails with a RuntimeError on `json.loads`. Here
/// the second line of defence is checked — valid JSON, but NOT an object: the core must
/// consider it neither an RPC error nor results.
#[test]
fn non_object_response_yields_nothing() {
    for raw in [
        "[]",
        "[{\"Name\": \"x\"}]",
        "\"error\"",
        "null",
        "42",
    ] {
        let data: Value = serde_json::from_str(raw).unwrap();
        assert!(parse_results(&data).is_empty(), "parse_results {raw}");
        assert!(curated_order(&data).is_empty(), "curated_order {raw}");
        assert_eq!(check_rpc_error(&data), Ok(()), "check_rpc_error {raw}");
        assert!(
            search_filter(&data, "icon", None).is_empty(),
            "search_filter {raw}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Category relevance (03.10.2026): a live AUR RPC snapshot where broad
//  queries pulled programs and libraries into the catalog instead of themes.
// ═══════════════════════════════════════════════════════════════════════════

fn rpc(records: &[(&str, &str)]) -> Value {
    let results: Vec<Value> = records
        .iter()
        .map(|(n, d)| serde_json::json!({"Name": n, "Description": d, "NumVotes": 1}))
        .collect();
    serde_json::json!({ "results": results })
}

fn kept(key: &str, records: &[(&str, &str)]) -> Vec<String> {
    let data = rpc(records);
    let mut out: Vec<String> = dedup_category(&[&data], key).into_iter().map(|p| p.name).collect();
    out.sort();
    out
}

/// «Глобальные темы»: "look and feel" in the description does not make a Plasma theme yet.
#[test]
fn global_drops_look_and_feel_programs() {
    let got = kept(
        "global",
        &[
            ("aerothemeplasma-desktop-git", "An alternative shell for KDE Plasma that aims to replicate the look and feel of Windows 7"),
            ("aerothemeplasma-icons-git", "Icon theme designed for AeroThemePlasma"),
            ("aerothemeplasma-sounds-git", "Collection of sound themes designed for AeroThemePlasma"),
            ("cool-retro-term-git", "A terminal emulator which mimics the look and feel of the old cathode tube screens."),
            ("qemacs-git", "QEmacs (Quick Emacs) is a very small editor with Emacs look and feel"),
            ("windowmaker-git", "An X11 window manager with a NEXTSTEP look and feel"),
            ("labwc-git", "stacking wayland compositor with look and feel from openbox (git version)"),
            ("singularityviewer-alpha", "An exciting client for Second Life which combines the look and feel of Viewer 1.23"),
            ("jgoodies-looks", "A Java Swing look and feel library"),
            ("dtk6gui-git", "Deepin Toolkit, gui module for DDE look and feel"),
            ("klassy", "Highly customizable binary Window Decoration, Application Style and Global Theme plugin for recent versions of the KDE Plasma desktop."),
            ("utterly-nord-plasma", "A Slick and Modern Global theme for KDE Plasma"),
            ("mytm", "Global theme manager plugin for MyCTL"),
            ("plasma-theme-master", "Daemon and GUI to generate/synchronize and control theme dynamically"),
            ("whitesur-plasma-theme-git", "macOS Big Sur-like KDE Plasma theme"),
        ],
    );
    assert_eq!(
        got,
        ["aerothemeplasma-desktop-git", "klassy", "utterly-nord-plasma", "whitesur-plasma-theme-git"]
    );
}

/// AeroThemePlasma icons and sounds — in their own sections.
#[test]
fn aerothemeplasma_parts_live_in_own_sections() {
    let icons = kept("icons", &[("aerothemeplasma-icons-git", "Icon theme designed for AeroThemePlasma")]);
    assert_eq!(icons, ["aerothemeplasma-icons-git"]);
    let sounds = kept(
        "sounds",
        &[
            ("aerothemeplasma-sounds-git", "Collection of sound themes designed for AeroThemePlasma"),
            ("yaru-sound-theme", "Yaru default ubuntu sound theme"),
            ("freeswitch-sounds-ru-ru-8000", "Voice recordings for FreeSWITCH"),
            ("ctr-popsoundshaper-vst3", "Cut Through Recordings Pop Sound Filter - Dynamic Filter (VST3)"),
            ("libcanberra-gtk2", "A small and lightweight implementation of the XDG Sound Theme Specification"),
            ("xdg-sound", "Command line XDG sound theme utilities"),
            ("plasma-mobile-sounds", "Sounds for Plasma Mobile"),
        ],
    );
    assert_eq!(sounds, ["aerothemeplasma-sounds-git", "plasma-mobile-sounds", "yaru-sound-theme"]);
}

/// Color schemes, icons, cursors, wallpapers: the tails of broad queries.
#[test]
fn other_categories_drop_non_themes() {
    let colors = kept(
        "colors",
        &[
            ("catppuccin-plasma-colorscheme-mocha", "Soothing pastel theme for KDE Plasma"),
            ("vim-colorscheme-alduin", "A dark low-contrast Vim colorscheme"),
            ("catppuccin-konsole-colorscheme-mocha-git", "Soothing pastel theme for Konsole"),
            ("alacritty-colorscheme", "Change colorscheme of alacritty with ease"),
            ("papirus-color-scheme", "Papirus color scheme for KDE"),
        ],
    );
    assert_eq!(colors, ["catppuccin-plasma-colorscheme-mocha", "papirus-color-scheme"]);
    let icons = kept(
        "icons",
        &[
            ("akonadiconsole-git", "Akonadi management and debugging console"),
            ("ttf-ionicons", "Font from the Ionic mobile framework"),
            ("python-faicons", "An interface to Font-Awesome for use in Shiny"),
            ("mint-y-icons", "A flat, colorful, and modern theme based on Paper and Moka"),
            ("foxicons-theme", "A collection of multi-sized icons organized as a Linux desktop icon theme"),
            ("geticons", "CLI tool to get icons on Linux and other Freedesktop systems"),
        ],
    );
    assert_eq!(icons, ["foxicons-theme", "mint-y-icons"]);
    let cursors = kept(
        "cursor",
        &[
            ("emacs-multiple-cursors", "Multiple cursors for Emacs"),
            ("python-mplcursors", "Interactive data selection cursors for Matplotlib"),
            ("phinger-cursors", "Most likely the most over-engineered cursor theme."),
        ],
    );
    assert_eq!(cursors, ["phinger-cursors"]);
    let walls = kept(
        "wallpapers",
        &[
            ("arch-wallpapers-git", "Arch wallpapers"),
            ("xwallpaper-git", "Wallpaper setting utility for X"),
            ("linux-wallpaperengine-git", "use steam's wallpaperengine on linux"),
            ("plasma6-wallpapers-komplex", "Use complex shader arrangements as a desktop background"),
            ("plasma6-themes-matchama-wallpapers", "Matchama wallpapers"),
        ],
    );
    assert_eq!(walls, ["arch-wallpapers-git", "plasma6-themes-matchama-wallpapers"]);
}

/// Wallpapers: background changing and downloading programs, GNOME extensions, Plasma
/// plugins and a GTK theme variant (live snapshot 04.10.2026).
#[test]
fn wallpapers_drop_changers_and_downloaders() {
    let walls = kept(
        "wallpapers",
        &[
            ("ubuntu-wallpapers", "The default Wallpapers for Ubuntu"),
            ("gnome-shell-extension-bing-wallpaper", "Changes your wallpaper daily to the bing.com background image"),
            ("bing-wallpaper-git", "Bing Wallpaper of the Day as your Linux Desktop's wallpaper"),
            ("nasa-wallpaper", "Change your desktop background with a NASA image."),
            ("reddit-wallpaper-fetcher-git", "Automatically fetch images from reddit for a wallpaper"),
            ("plasma-wallpaper-snow-git", "Plasma Wallpaper Snow from Ivan Safonov"),
            ("graphite-gtk-theme-wallpaper-git", "Graphite gtk theme"),
            ("doomfire-wallpaper", "Animated doom fire effect wallpaper for Wayland compositors"),
            ("dynamic-wallpaper", "Dynamic wallpaper creator for Gnome 42"),
            ("dynamic-wallpaper-mojave-images-git", "macOS Mojave dynamic wallpaper based 16 images"),
        ],
    );
    assert_eq!(walls, ["dynamic-wallpaper-mojave-images-git", "ubuntu-wallpapers"]);
    let icons = kept(
        "icons",
        &[
            ("gnome-shell-extension-desktop-icons", "Add icons to the desktop"),
            ("themix-icons-papirus-git", "Papirus icons plugin for Themix GUI designer"),
            ("nautilus-folder-icons", "Nautilus extension that makes changing folders icons easy!"),
            ("adwaita-xfce-icon-theme", "This icon theme is an extension for Adwaita fixing missing icons"),
        ],
    );
    assert_eq!(icons, ["adwaita-xfce-icon-theme"]);
    let cursors = kept(
        "cursor",
        &[
            ("android-x86-libxcursor", "X cursor management library (Android x86)"),
            ("xcursor-viewer-git", "view xcursor files in list"),
            ("xcursor-breeze-neutral-git", "Neutral fork of KDEs Breeze Cursor"),
        ],
    );
    assert_eq!(cursors, ["xcursor-breeze-neutral-git"]);
}

/// GTK themes: until 04.10.2026 the section had no queries, and AUR showed
/// only the selection. The queries also bring foreign things — icons "for a GTK theme",
/// cursors, Fluxbox styles, theme switchers; themes without "gtk" in the name
/// (found through a dependency on Murrine) stay.
#[test]
fn gtk_section_keeps_themes_only() {
    assert!(!rexon_core::aur::category_queries("gtk").is_empty());
    let got = kept(
        "gtk",
        &[
            ("arc-gtk-theme", "A flat theme with transparent elements for GTK 2,3,4 and Gnome-Shell"),
            ("zuki-themes", "Zuki themes for GNOME, Xfce and more."),
            ("ubuntu-themes", "Ubuntu monochrome icon theme, Ambiance and Radiance themes, and Ubuntu artwork"),
            ("flat-remix-gtk", "Flat Remix GTK theme is a pretty simple gtk window theme"),
            ("gtk-theme-plasma-git", "morning_star gtk themes collection"),
            ("gtk-theme-equinox", "Official themes for Equinox GTK engine."),
            ("gtk-theme-config", "A tool to configure GTK theme colors."),
            ("gtk-theme-switch2", "Gtk2 theme switcher"),
            ("sonic-gtk-theme-bridge", "Syncs SonicDE settings to GTK applications"),
            ("wpgtk-git", "A gui wallpaper chooser that changes your WM theme, GTK theme and more"),
            ("faenza-icon-theme", "Icon theme designed for Equinox GTK theme"),
            ("layan-cursor-theme-git", "A x-cursor theme inspired by the Layan GTK Theme"),
            ("absolute-theme", "A light fluxbox theme that fits well with absolute gtk theme"),
            ("windows10-tint2rc", "tin2rc file for windows10 gtk theme"),
            ("mactahoe-plasma-theme-git", "Mactahoe KDE theme. This package includes plasma and gtk themes"),
            ("gtk-engine-rezlooks", "Clean looking gtk theme engine"),
            ("themix-theme-materia-git", "Materia theme plugin for Themix GUI designer"),
            ("lib32-gtk-engine-murrine", "GTK2 engine (32 bit)"),
            ("xlog", "Ham Radio general purpose logging program."),
        ],
    );
    assert_eq!(
        got,
        [
            "arc-gtk-theme",
            "flat-remix-gtk",
            "gtk-theme-equinox",
            "gtk-theme-plasma-git",
            "ubuntu-themes",
            "zuki-themes",
        ]
    );
}

// ── The CURATED selection in its sections ──────────────────────────────

fn pkg(name: &str, votes: i64) -> AurPackage {
    AurPackage { name: name.to_string(), votes, ..Default::default() }
}

/// Every curated package except the window decoration module `klassy`
/// has its own section; AeroThemePlasma is spread over three sections.
#[test]
fn every_curated_package_has_its_section() {
    use rexon_core::aur::{curated_category, has_curated, CURATED};
    let homeless: Vec<&str> = CURATED.iter().copied().filter(|n| curated_category(n).is_none()).collect();
    assert_eq!(homeless, ["klassy"]);
    assert_eq!(curated_category("aerothemeplasma-desktop-git"), Some("global"));
    assert_eq!(curated_category("aerothemeplasma-icons-git"), Some("icons"));
    assert_eq!(curated_category("aerothemeplasma-sounds-git"), Some("sounds"));
    for key in ["global", "icons", "sounds", "cursor", "gtk"] {
        assert!(has_curated(key), "{key}");
    }
    assert!(!has_curated("colors"));
    assert!(!has_curated("popular"));
}

/// Pinning: a section's curated packages are at the top in CURATED order, one found in
/// the results is not repeated, other sections' and ones gone from AUR are not added.
#[test]
fn pin_curated_puts_section_picks_on_top() {
    use rexon_core::aur::pin_curated;
    let curated = vec![
        pkg("aerothemeplasma-icons-git", 5),
        pkg("aerothemeplasma-sounds-git", 3),
        pkg("tela-icon-theme", 40),
        pkg("bibata-cursor-theme", 90),
    ];
    let list = vec![pkg("papirus-icon-theme", 900), pkg("tela-icon-theme", 41), pkg("x-icons", 1)];
    let got: Vec<String> = pin_curated("icons", &curated, list.clone()).into_iter().map(|p| p.name).collect();
    assert_eq!(
        got,
        ["aerothemeplasma-icons-git", "tela-icon-theme", "papirus-icon-theme", "x-icons"],
        "значки: свои наверх, tela — из выдачи и без повтора"
    );
    let sounds = pin_curated("sounds", &curated, vec![pkg("ocean-sound-theme", 2)]);
    assert_eq!(sounds[0].name, "aerothemeplasma-sounds-git");
    assert_eq!(sounds[0].category, "sounds", "ключ раздела — для значка плитки");
    assert_eq!(sounds.len(), 2);
    // A section without a selection — results as is.
    let colors: Vec<String> = pin_curated("colors", &curated, list).into_iter().map(|p| p.name).collect();
    assert_eq!(colors, ["papirus-icon-theme", "tela-icon-theme", "x-icons"]);
}
