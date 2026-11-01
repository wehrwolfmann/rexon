//! Golden-сверка AUR-парсера с оригиналом `rexon/appearance/aur_client.py`.
//!
//! Эталоны сняты с самого Python (см. генератор): `aur_search_raw.json` —
//! РЕАЛЬНЫЙ ответ RPC (`curl` search/aerothemeplasma), `aur_cases.json` —
//! выходы оракула (реальные `_parse`/атрибуты/`search_aur`/`fetch_for_category`
//! с монипатчем сети). Rust обязан вернуть те же модели/строки/порядок.
//!
//! ⚠️ Живой сетевой вызов НЕ тестируется (недетерминирован и в ядро не
//! портирован) — проверяется только РАЗБОР зафиксированного ответа.

use rexon_core::aur::{
    check_rpc_error, curated_order, dedup_category, parse_results, search_filter, AurPackage,
};
use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!("golden/aur_cases.json")).unwrap()
}

/// Разбор РЕАЛЬНОГО ответа RPC: все поля и вычисляемые атрибуты совпадают с
/// Python `_parse` + свойствами `AurPackage`.
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
        // вычисляемые
        assert_eq!(p.id(), w["id"].as_str().unwrap());
        assert_eq!(p.aur_url(), w["aur_url"].as_str().unwrap());
        assert_eq!(p.preview_url(), w["preview_url"].as_str().unwrap());
        assert_eq!(p.icon(), w["icon"].as_str().unwrap());
    }
}

/// preview_url: github-регэксп (owner/repo, .git и хвост отсекаются),
/// не-github → "".
#[test]
fn preview_url_matches_python() {
    for (url, want) in cases()["url_cases"].as_object().unwrap() {
        let p = AurPackage { upstream_url: url.clone(), ..Default::default() };
        assert_eq!(&p.preview_url(), want.as_str().unwrap(), "preview для {url}");
    }
}

/// icon: по категории, иначе эвристика по имени (порядок проверок как в Python).
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

/// aur_url: percent-encode имени как `urllib.parse.quote`.
#[test]
fn aur_url_matches_python() {
    for (name, want) in cases()["aururl_cases"].as_object().unwrap() {
        let p = AurPackage { name: name.clone(), ..Default::default() };
        assert_eq!(&p.aur_url(), want.as_str().unwrap(), "aur_url для {name}");
    }
}

/// search_aur: локальный AND-фильтр многословного запроса + sort_votes.
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

/// fetch_for_category("global"): дедуп по имени (первый выигрывает),
/// sort_votes, пин aerothemeplasma наверх; category проставлена.
/// `aerothemeplasma-icons-git` с 03.10.2026 НЕ в «Глобальных темах» — он
/// живёт в «Значках» (`aur::is_relevant`).
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
    // category у каждого = "global"
    for p in &out {
        assert_eq!(p.category, key);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Закрытие дыр паритета: курируемый список, «поле не того типа», ошибки RPC
//  и кривые/обрезанные ответы. Эталоны сняты с живого aur_client.py при
//  Python-путь до его удаления (генератор — ad-hoc, вход указан в каждом тесте).
// ═══════════════════════════════════════════════════════════════════════════

/// `fetch_curated` на РЕАЛЬНОМ ответе AUR: пин `aerothemeplasma*` идёт в
/// порядке `CURATED`.
///
/// ⚠️ Тест закрепляет КВИРК оригинала: пакеты `aerothemeplasma-*`, которых нет
/// в `CURATED` (`-desktop-x11-git` в живом ответе), выпадают из выдачи вообще
/// — в пин они не попадают (нет в списке), а из «остальных» отфильтрованы по
/// префиксу. Поведение сохранено осознанно: подборка показывает ровно то, что
/// в `CURATED`. `-sounds-git` с 03.10.2026 в `CURATED` и закреплён третьим.
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

/// `fetch_curated` на смешанном ответе: пин по порядку `CURATED`, остальные по
/// голосам (убыв.), запись без `Name` пропущена (оригинал падал KeyError).
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

/// «Поле не того типа»: числа строками/дробными/bool, мусор и null дают ровно
/// те же значения, что починенный Python (`_num`), без паники.
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

/// Ответ с ошибкой RPC (в т.ч. реальный формат `type=error`) распознаётся с тем
/// же текстом, что у Python; нормальный ответ ошибкой не считается.
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

/// Кривые и обрезанные ответы: `{}`, `results: null`, строка вместо массива,
/// не-объекты внутри массива. Разбор обязан вернуть то же, что починенный
/// Python, и НИ В КАКОМ случае не паниковать.
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

        // search_filter по единственному слову — тот же путь, что _py_search_aur.
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

/// Тело ответа — HTML (страница ошибки/капчи вместо JSON). До ядра такое не
/// доходит: `_http_json` валит RuntimeError'ом на `json.loads`. Здесь
/// проверяется вторая линия — валидный JSON, но НЕ объект: ядро не должно
/// считать его ни ошибкой RPC, ни выдачей.
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
//  Релевантность категорий (03.10.2026): живой снимок AUR RPC, где широкие
//  запросы тянули в каталог программы и библиотеки вместо тем.
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

/// «Глобальные темы»: «look and feel» в описании ещё не тема Plasma.
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

/// Значки и звуки AeroThemePlasma — в своих разделах.
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

/// Цветовые схемы, значки, курсоры, обои: хвосты широких запросов.
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

/// Обои: программы смены и скачивания фона, дополнения GNOME, плагины
/// Plasma и вариант темы GTK (живой снимок 04.10.2026).
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

/// Темы GTK: до 04.10.2026 у раздела не было запросов, и AUR показывал
/// только подборку. Запросы приносят и чужое — значки «под тему GTK»,
/// курсоры, стили Fluxbox, переключатели тем; темы без «gtk» в имени
/// (найденные по зависимости от Murrine) остаются.
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

// ── Подборка CURATED в своих разделах ──────────────────────────────────

fn pkg(name: &str, votes: i64) -> AurPackage {
    AurPackage { name: name.to_string(), votes, ..Default::default() }
}

/// У каждого курируемого пакета, кроме модуля оформления окон `klassy`,
/// есть свой раздел; AeroThemePlasma разложена по трём разделам.
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

/// Закрепление: курируемые раздела — наверху в порядке CURATED, найденный в
/// выдаче не повторяется, чужие разделы и пропавшие из AUR не добавляются.
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
    // Раздел без подборки — выдача как есть.
    let colors: Vec<String> = pin_curated("colors", &curated, list).into_iter().map(|p| p.name).collect();
    assert_eq!(colors, ["papirus-icon-theme", "tela-icon-theme", "x-icons"]);
}
