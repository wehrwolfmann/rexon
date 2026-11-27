//! Golden check of the `installed_scan.rs` port against the original
//! `rexon/appearance/installed_scan.py`.
//!
//! The goldens were captured from Python itself using the "sandbox + snapshot" scheme:
//!   1. A one-off generator took a snapshot of the REAL theme tree of
//!      Wehrwolfmann's machine (~/.local/share/plasma, /usr/share/themes, icons,
//!      cursors, Kvantum, sddm, plymouth — only the structure + metadata
//!      files, binary previews as dummies; paths normalized to
//!      /sb/home and /sb/usr) and added synthetic cases: a junk look-and-feel without
//!      metadata, a broken metadata.json, a theme with an empty Name, metadata.desktop
//!      with and without Name, a cursor theme in icons/, a color scheme with a space in
//!      its name, a sidecar preview, backfill cases from the store cache
//!      (an unambiguous and an ambiguous match).
//!   2. The snapshot was materialized into a REAL sandbox — the Python golden ran
//!      over the real os.scandir/open (the file layer was not mocked), and
//!      `snapshot` in the fixture keeps the order of names as os.scandir returned
//!      them to the golden → Rust iterates in the same order.
//!   3. What cannot be mocked in Python: `_active_map`/`_qstyle_factory`/pacman/the store
//!      cache were replaced by fixtures — in Rust these are scanner parameters.
//!
//! ⚠️ The snapshot is TRUNCATED in width (each root has about fifteen themes out of
//! real hundreds), but honest in depth: the texts of `metadata.json`,
//! `metadata.desktop`, `index.theme` and `*.colors` lie in it VERBATIM
//! as on Wehrwolfmann's machine. The card label is taken from them,
//! so an empty text would mean "the theme has no name" and the golden
//! would stop proving anything about icons, cursors, GTK themes and
//! color schemes.
//!
//! `golden/installed_scan_tree.json` — the snapshot + scanner goldens
//! (dataclasses.asdict FIELD-BY-FIELD, including the order of items).
//! `golden/installed_scan_pure.json` — pure parsers: name normalization,
//! sidecar keys, the `pacman -Qm` policy (retries on db.lck), parsing of
//! `pacman -Qo`, building `_active_map` from ready sources (Python ran
//! with mocked ThemeManager/kreadconfig6/configs).

use std::collections::{HashMap, HashSet};

use rexon_core::installed_scan::{
    active_map_from_sources, backfill_preview, foreign_pkgs_from_attempts,
    normalize_theme_name, parse_qo_output, scan_active, scan_downloaded,
    scan_kvantum_themes, scan_lnf_component, scan_system, scan_widget_styles,
    sidecar_key, ActiveMapSources, FsSnapshot, QmAttempt, ScanEnv,
};
use rexon_core::system::PkgFamily;
use rexon_core::system_plan::{file_owner_queries, foreign_packages_query};
use serde_json::{json, Value};

/// The language the goldens were captured in: our interface is Russian, and the theme labels
/// in the golden are those the buyer will see. Pinned to the fixture on purpose —
/// otherwise the lists would depend on the locale of the machine running the tests.
const FX_LOCALE: &str = "ru_RU.UTF-8";

// Card labels (theme kind, «✅ Active», «⬇ Downloaded») are translated, and
// the goldens were captured from the Russian interface: the checks run on the Russian catalog
// `po/ru.po` and at the same time keep the promise that in Russian everything is as before.
mod i18n_pin;

fn tree_fx() -> Value {
    serde_json::from_str(include_str!("golden/installed_scan_tree.json")).unwrap()
}

fn pure_fx() -> Value {
    serde_json::from_str(include_str!("golden/installed_scan_pure.json")).unwrap()
}

fn amap(v: &Value) -> HashMap<String, String> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, val)| (k.clone(), val.as_str().unwrap().to_string()))
        .collect()
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// Environment from the fixture. Returns the preview_file closure separately,
/// so that the lifetimes of the references in ScanEnv are correct.
struct FxEnv {
    fs: FsSnapshot,
    ocs_cache: Vec<(String, String)>,
    preview_files: HashMap<String, String>,
    qstyles: Vec<String>,
    amap_full: HashMap<String, String>,
    amap_qt: HashMap<String, String>,
    cases: Value,
}

fn load_env() -> FxEnv {
    let fx = tree_fx();
    let fs = FsSnapshot::from_json(&fx["snapshot"]);
    let ocs_cache: Vec<(String, String)> = fx["ocs_cache"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            let a = pair.as_array().unwrap();
            (
                a[0].as_str().unwrap().to_string(),
                a[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let preview_files: HashMap<String, String> = fx["preview_files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    FxEnv {
        fs,
        ocs_cache,
        preview_files,
        qstyles: str_vec(&fx["qstyles"]),
        amap_full: amap(&fx["amap_full"]),
        amap_qt: amap(&fx["amap_qt"]),
        cases: fx["cases"].clone(),
    }
}

fn items_json(items: &[rexon_core::models::ThemeItem]) -> Value {
    serde_json::to_value(items).unwrap()
}

// ============================================================================
// Scanners over the snapshot
// ============================================================================

#[test]
fn scanners_match_python() {
    let _lang = i18n_pin::ru();
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: FX_LOCALE,
        standard_icons: None,
    };
    let empty: HashMap<String, String> = HashMap::new();

    assert_eq!(
        items_json(&scan_active(&env, &e.amap_full)),
        e.cases["scan_active_full"],
        "scan_active(full)"
    );
    assert_eq!(
        items_json(&scan_active(&env, &empty)),
        e.cases["scan_active_empty"],
        "scan_active(empty)"
    );
    assert_eq!(
        items_json(&scan_downloaded(&env, &e.amap_full)),
        e.cases["scan_downloaded_full"],
        "scan_downloaded(full)"
    );
    assert_eq!(
        items_json(&scan_downloaded(&env, &empty)),
        e.cases["scan_downloaded_empty"],
        "scan_downloaded(empty)"
    );

    // widget styles / kvantum / lnf
    assert_eq!(
        items_json(&scan_widget_styles(&e.amap_full, &e.qstyles, true)),
        e.cases["scan_widget_qt_only"],
        "scan_widget_styles(qt_only)"
    );
    assert_eq!(
        items_json(&scan_widget_styles(&e.amap_qt, &e.qstyles, false)),
        e.cases["scan_widget_all"],
        "scan_widget_styles(all)"
    );
    assert_eq!(
        items_json(&scan_kvantum_themes(&env, &e.amap_full)),
        e.cases["scan_kvantum_on"],
        "scan_kvantum(on)"
    );
    assert_eq!(
        items_json(&scan_kvantum_themes(&env, &e.amap_qt)),
        e.cases["scan_kvantum_off"],
        "scan_kvantum(off)"
    );
    // The last splash screen tile is «None» (it is not in the golden: the label
    // is translated and checked separately, `splash_none_tile_is_last`).
    let mut splash = scan_lnf_component(&env, "splash", &e.amap_full);
    assert_eq!(splash.pop().map(|it| it.id), Some("local:splash:None".to_string()));
    assert_eq!(items_json(&splash), e.cases["scan_lnf_splash"], "scan_lnf(splash)");
    assert_eq!(
        items_json(&scan_lnf_component(&env, "lockscreen", &e.amap_full)),
        e.cases["scan_lnf_lockscreen"],
        "scan_lnf(lockscreen)"
    );
}

/// The human name is visible ONLY to the buyer: the card number, the install path
/// and the active theme mark stayed on the directory names.
///
/// This also checks that list search keeps working — it
/// filters by what a person reads on the card (`local_matches`), and
/// after replacing the label it must find the theme by its REAL name.
#[test]
fn human_name_is_shown_but_identity_stays_on_directory() {
    let _lang = i18n_pin::ru();
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: FX_LOCALE,
        standard_icons: None,
    };

    let splash = scan_lnf_component(&env, "splash", &e.amap_full);
    let ours = splash
        .iter()
        .find(|it| it.id == "local:splash:org.rexon.sessionsplash")
        .expect("наша заставка обязана быть в списке");
    assert_eq!(ours.name, "Rexon Session Splash", "подпись для человека");
    assert_eq!(
        ours.install_path,
        "/sb/home/.local/share/plasma/look-and-feel/org.rexon.sessionsplash",
        "путь установки трогать нельзя"
    );

    // The active theme is marked by directory name, not by label.
    let active = splash
        .iter()
        .find(|it| it.local_kind.as_deref() == Some("active"))
        .expect("одна из тем активна");
    let dir = e.amap_full.get("splash").expect("активная заставка задана");
    assert_eq!(
        active.id,
        format!("local:splash:{dir}"),
        "активной помечена не та тема"
    );
    assert_ne!(active.name, *dir, "у активной темы имя тоже человеческое");
    assert!(active.author.ends_with("✅ Активна"), "бейдж: {}", active.author);
    assert_eq!(
        splash.iter().filter(|it| it.local_kind.is_some()).count(),
        1,
        "признак «активна» — только у одной темы"
    );

    // The name for applying is still taken from the card number.
    assert_eq!(
        rexon_core::appearance_page::local_theme_name(&ours.id, &ours.name),
        "org.rexon.sessionsplash"
    );

    // Search goes by the label — the buyer searches for what they see.
    assert!(rexon_core::appearance_page::local_matches(&ours.name, "session"));
    let found = rexon_core::appearance_page::local_filter(&splash, "rexon session");
    assert_eq!(found.len(), 1, "поиск по человеческому имени: {found:?}");
    assert_eq!(found[0].id, ours.id);
}

/// Icons, cursors, GTK themes and color schemes are also labelled in a human way.
///
/// They have no KDE metadata: the name lies in `index.theme` (icons, cursors,
/// GTK) or inside the `*.colors` file itself. Exactly the same is checked as for
/// look-and-feel: the label is human, while the card number, the install path
/// and the name for applying stayed on the directory/file name.
#[test]
fn human_name_reaches_icons_cursors_gtk_and_colors() {
    let _lang = i18n_pin::ru();
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: FX_LOCALE,
        standard_icons: None,
    };
    let case = &e.cases["scan_system"];
    let foreign: HashSet<String> = str_vec(&case["foreign"]).into_iter().collect();
    let mut owners: HashMap<String, String> = case["owners"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    // A GTK theme with a translated name: it is not in the golden owner map.
    owners.insert("/sb/usr/share/themes/BlueMenta".to_string(), "mate-themes".to_string());
    let scan = scan_system(&env, &e.amap_full, true, Some(&foreign), &owners)
        .expect("scan_system ok");

    // kind → (card number, expected label, install path)
    let want = [
        // cursor: the directory name and the theme name are DIFFERENT, and translated as well
        ("local:cursor:breeze_cursors", "Breeze, тёмный вариант",
         "/sb/usr/share/icons/breeze_cursors"),
        ("local:cursor:Oxygen_Black", "Oxygen, чёрный вариант",
         "/sb/usr/share/icons/Oxygen_Black"),
        // icons: the name is not translated, but is spelled differently from the directory
        ("local:icons:Cosmic", "COSMIC", "/sb/usr/share/icons/Cosmic"),
        // GTK theme: the name in [Desktop Entry] of index.theme
        ("local:gtk:BlueMenta", "Menta (Синяя)",
         "/sb/usr/share/themes/BlueMenta"),
        // color scheme: no directory at all, the name is inside the *.colors file
        ("local:colors:BreezeClassic", "Breeze, классический вариант",
         "/sb/usr/share/color-schemes/BreezeClassic"),
        ("local:colors:IridescentLightly", "Iridescent Lightly",
         "/sb/usr/share/color-schemes/IridescentLightly"),
    ];
    for (id, display, path) in want {
        let it = scan
            .items
            .iter()
            .find(|it| it.id == id)
            .unwrap_or_else(|| panic!("нет карточки {id}"));
        assert_eq!(it.name, display, "подпись для человека у {id}");
        assert_eq!(it.install_path, path, "путь установки у {id} трогать нельзя");
        // The name for applying the theme is still taken from the card number,
        // not from the label: otherwise plasma-apply-* would get «COSMIC».
        let dir = id.rsplit(':').next().unwrap();
        assert_eq!(
            rexon_core::appearance_page::local_theme_name(&it.id, &it.name),
            dir,
            "имя для применения у {id}"
        );
        // Search goes by the label — the buyer searches for what they see.
        assert!(
            rexon_core::appearance_page::local_matches(&it.name, display),
            "поиск по подписи {display:?}"
        );
    }

    // …and by what the theme is called in the original: the untranslated name (`Name=`) and
    // the directory name. Live check r1434: «Adwaita» and «HighContrast» were not
    // found, because the cards say «Адвайта» and
    // «Высококонтрастная».
    let by_id = |id: &str| scan.items.iter().find(|it| it.id == id).unwrap().clone();
    // kind → (card number, untranslated name, queries that must find it)
    let want_source = [
        ("local:cursor:breeze_cursors", "Breeze Dark",
         &["breeze dark", "BREEZE_CURSORS", "breezecursors"][..]),
        ("local:cursor:Oxygen_Black", "Oxygen Black", &["oxygen black", "Oxygen_Black"][..]),
        ("local:gtk:BlueMenta", "BlueMenta", &["BlueMenta", "blue menta", "bluementa"][..]),
        ("local:colors:BreezeClassic", "Breeze Classic",
         &["breeze classic", "BreezeClassic"][..]),
    ];
    for (id, source, queries) in want_source {
        let it = by_id(id);
        assert_eq!(it.source_name, source, "имя без перевода у {id}");
        for q in queries {
            let found = rexon_core::appearance_page::local_filter(&scan.items, q);
            assert!(
                found.iter().any(|f| f.id == id),
                "запрос {q:?} обязан найти {id}: {:?}",
                found.iter().map(|f| &f.id).collect::<Vec<_>>()
            );
        }
    }
    // An untranslated name equal to the label is not duplicated.
    assert_eq!(by_id("local:colors:IridescentLightly").source_name, "");
    // Original names do not blur the search: another theme is not found by them.
    let found = rexon_core::appearance_page::local_filter(&scan.items, "bluementa");
    assert!(found.iter().all(|f| f.id.contains("BlueMenta")), "лишнее: {found:?}");
}

#[test]
fn scan_system_matches_python() {
    let _lang = i18n_pin::ru();
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: FX_LOCALE,
        standard_icons: None,
    };
    let case = &e.cases["scan_system"];
    let foreign: HashSet<String> = str_vec(&case["foreign"]).into_iter().collect();
    let owners: HashMap<String, String> = case["owners"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();

    let scan = scan_system(&env, &e.amap_full, true, Some(&foreign), &owners)
        .expect("scan_system ok");
    assert_eq!(json!(scan.qo_paths), case["qo_paths"], "пути в pacman -Qo");
    assert_eq!(items_json(&scan.items), case["items"], "элементы");

    // No pacman → empty (outside Arch the classification is impossible).
    let no_pacman = scan_system(&env, &e.amap_full, false, Some(&foreign), &owners)
        .expect("без pacman — Ok");
    assert_eq!(
        items_json(&no_pacman.items),
        e.cases["scan_system_no_pacman"],
        "без pacman"
    );

    // pacman -Qm unavailable → SystemScanUnavailable (text 1-to-1).
    let err = scan_system(&env, &e.amap_full, true, None, &owners)
        .expect_err("должен быть Err");
    assert_eq!(
        json!(err),
        e.cases["scan_system_unavailable_message"],
        "текст SystemScanUnavailable"
    );
}

// ============================================================================
// Pure parsers
// ============================================================================

#[test]
fn normalize_and_sidecar_match_python() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    for (name, want) in fx["normalize"].as_object().unwrap() {
        assert_eq!(
            json!(normalize_theme_name(name)),
            *want,
            "normalize({name:?})"
        );
    }
    for (key, want) in fx["sidecar_key"].as_object().unwrap() {
        let (kind, name) = key.split_once('|').unwrap();
        assert_eq!(json!(sidecar_key(kind, name)), *want, "sidecar({key:?})");
    }
}

#[test]
fn backfill_preview_matches_python() {
    let _lang = i18n_pin::ru();
    // Backfill is also checked inside the scanners (WhiteSur-Dark in downloaded);
    // here — point cases of unambiguity.
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: FX_LOCALE,
        standard_icons: None,
    };
    // An unambiguous match → a local file from the cache (the goldens were captured from live
    // Python: _backfill_preview on this same cache).
    assert_eq!(
        backfill_preview(&env, "WhiteSur-Dark"),
        "/sb/home/.cache/rexon/previews/whitesur.png"
    );
    // "nord" ⊂ "nordsomething", but "nordic" is not → the match is UNAMBIGUOUS (only
    // nord-a) and the preview is substituted. Python confirmed: the same path.
    assert_eq!(
        backfill_preview(&env, "Nord-something"),
        "/sb/home/.cache/rexon/previews/whitesur.png"
    );
    // Ambiguous: "nordic" matches both "Nord" (nord-a) and "Nordic Theme"
    // (nord-b) → 2 URLs → empty (Python confirmed).
    assert_eq!(backfill_preview(&env, "Nordic"), "");
    // A short stem (<4) → empty.
    assert_eq!(backfill_preview(&env, "abc"), "");
}

#[test]
fn foreign_pkgs_policy_matches_python() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    for (label, case) in fx["foreign_pkgs"].as_object().unwrap() {
        let attempts: Vec<QmAttempt> = case["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let arr = a.as_array().unwrap();
                match arr[0].as_str().unwrap() {
                    "ok" => QmAttempt::Ok(arr[1].as_str().unwrap().to_string()),
                    "fail" => QmAttempt::Fail(arr[1].as_str().unwrap().to_string()),
                    _ => QmAttempt::Error,
                }
            })
            .collect();
        let (result, used) = foreign_pkgs_from_attempts(true, &attempts);
        let got = match result {
            Some(set) => {
                let mut v: Vec<String> = set.into_iter().collect();
                v.sort();
                json!(v)
            }
            None => Value::Null,
        };
        assert_eq!(got, case["result"], "{label}: результат");
        assert_eq!(json!(used), case["used"], "{label}: число попыток");
    }
    // Outside Arch — an empty set without attempts.
    let (res, used) = foreign_pkgs_from_attempts(false, &[]);
    assert_eq!(res, Some(HashSet::new()));
    assert_eq!(used, 0);
}

#[test]
fn qo_parser_matches_python() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    let case = &fx["owner_pkgs"];
    let got = parse_qo_output(case["stdout"].as_str().unwrap());
    let want: HashMap<String, String> = case["result"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(got, want, "разбор ответа о владельце файла");
}

/// This machine's live answer, an empty answer, the answer of a command that did not run and
/// garbage in the output — on one and the same parser.
#[test]
fn qo_parser_extra_cases_match_golden() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    let cases = fx["owner_pkgs_cases"].as_array().expect("owner_pkgs_cases[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let got = parse_qo_output(case["stdout"].as_str().unwrap());
        let want: HashMap<String, String> = case["expected"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        assert_eq!(got, want, "{label}: разбор ответа о владельце файла");
    }
}

/// There is ONE command for the whole project: the argv recorded in the scan golden next to
/// the answer (`calls`) must match what the package engine returns.
/// If it diverges, a second copy of the command has crept in somewhere again.
#[test]
fn pkg_origin_calls_come_from_the_engine() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    let want_qm: Vec<String> = foreign_packages_query(PkgFamily::Arch)
        .expect("Arch отвечает")
        .cmd
        .argv;
    for (label, case) in fx["foreign_pkgs"].as_object().unwrap() {
        for call in case["calls"].as_array().unwrap() {
            assert_eq!(
                json!(want_qm),
                *call,
                "{label}: argv в голдене разошёлся с движком"
            );
        }
    }
    let case = &fx["owner_pkgs"];
    let paths: Vec<String> = case["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    let argvs: Vec<Vec<String>> = file_owner_queries(PkgFamily::Arch, &paths)
        .expect("Arch отвечает")
        .into_iter()
        .map(|q| q.cmd.argv)
        .collect();
    assert_eq!(
        json!(argvs),
        case["calls"],
        "argv владельцев в голдене разошёлся с движком"
    );
}

#[test]
fn active_map_matches_python() {
    let _lang = i18n_pin::ru();
    let fx = pure_fx();
    for case in fx["active_map"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let i = &case["inputs"];
        let s = |k: &str| i[k].as_str().unwrap_or("");
        let opt = |k: &str| i[k].as_str();
        let confs = |k: &str| -> Vec<&str> {
            i[k].as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect()
        };
        let src = ActiveMapSources {
            kde_global: s("kde_global"),
            kde_plasma_style: s("kde_plasma_style"),
            kde_colors: s("kde_colors"),
            icons: s("icons"),
            mouse_cursor: s("mouse_cursor"),
            kcminput_cursor: s("kcminput_cursor"),
            xcursor_env: s("xcursor_env"),
            gtk_settings_ini: opt("gtk_settings_ini"),
            widget_style: s("widget_style"),
            splash: opt("splash"),
            lockscreen: opt("lockscreen"),
            kwin_decoration_theme: opt("kwin_decoration_theme"),
            // The Python golden did not read `library`: None — the old rule by `theme=`.
            kwin_decoration_library: None,
            kvantum_kvconfig: opt("kvantum_kvconfig"),
            sddm_confs: confs("sddm_confs"),
            plymouth_confs: confs("plymouth_confs"),
            // The Python golden did not know the Qt style sources outside KDE at all (there it
            // read only kdeglobals): empty means the behaviour is as before.
            qt_style_override_env: s("qt_style_override_env"),
            qt_style_env_file: opt("qt_style_env_file"),
            lxqt_conf: opt("lxqt_conf"),
            qtct_conf: opt("qtct_conf"),
            // The Python golden did not know the environment's native store either: empty —
            // the old kdeglobals/settings.ini chain.
            native_gtk: None,
            native_icons: None,
            native_cursor: None,
            // The Python golden did not know sound and wallpapers either: the old cases have no such
            // sources, and the map must stay as before.
            sound_theme: opt("sound_theme"),
            wallpaper_current: opt("wallpaper_current"),
            home: s("home"),
            usr_root: s("usr_root"),
        };
        assert_eq!(
            Value::Object(active_map_from_sources(&src)),
            case["result"],
            "{label}: active_map"
        );
    }
}

/// Sound and wallpapers in the active theme map: the two new kinds are computed by the same map
/// as all the others — there is no separate path for them.
#[test]
fn sound_and_wallpaper_in_active_map() {
    let _lang = i18n_pin::ru();
    let base = ActiveMapSources::default;
    let key = |src: &ActiveMapSources, k: &str| -> String {
        active_map_from_sources(src)
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    // Sound: the command's answer arrives as is, gsettings quotes are stripped.
    let snd = ActiveMapSources { sound_theme: Some("'zorinos2'\n"), ..base() };
    assert_eq!(key(&snd, "sounds"), "zorinos2");
    // Nothing to read with — there is no line in the map at all (rather than an empty value).
    assert!(active_map_from_sources(&base()).get("sounds").is_none());
    // The theme is not set (empty answer) — there is no applied sound theme. Previously
    // an empty answer was replaced by `freedesktop`, and in the «Appearance» tab
    // an unselected theme showed up as applied.
    let empty = ActiveMapSources { sound_theme: Some("  \n"), ..base() };
    assert!(active_map_from_sources(&empty).get("sounds").is_none());

    // Wallpapers: the map gets the name of the PACK that contains the current picture.
    let home = "/sb/home";
    let usr = "/sb/usr";
    let wp = |cur: &'static str| -> String {
        key(
            &ActiveMapSources {
                wallpaper_current: Some(cur),
                home,
                usr_root: usr,
                ..base()
            },
            "wallpapers",
        )
    };
    assert_eq!(
        wp("/sb/home/.local/share/wallpapers/Пейзажи/contents/images/1920x1080.png"),
        "Пейзажи"
    );
    // A single picture lies right in the wallpaper directory — the pack name is its name,
    // exactly as it is recorded in the journal too.
    assert_eq!(wp("/sb/home/.local/share/wallpapers/Горы.png"), "Горы.png");
    // A system pack.
    assert_eq!(wp("/sb/usr/share/wallpapers/Next/"), "Next");
    // gsettings returns a percent-encoded URI — we decode it, otherwise Cyrillic would not
    // match any pack.
    assert_eq!(
        wp("'file:///sb/home/.local/share/wallpapers/%D0%9F%D0%B0%D1%87%D0%BA%D0%B0/a.png'"),
        "Пачка"
    );
    // A user's own file outside the wallpaper directories does not count as a pack.
    assert_eq!(wp("/sb/home/Изображения/своё.png"), "");
}

/// The «Active» badge on a Kvantum theme must work not only in KDE.
///
/// Activity is determined by widgetStyle, and outside KDE it is not taken from
/// kdeglobals: the session variable, our environment.d file, lxqt.conf, the
/// qt6ct config. While there was only one source, the badge never appeared outside KDE.
#[test]
fn widget_style_is_read_from_every_desktop_source() {
    let _lang = i18n_pin::ru();
    let base = ActiveMapSources::default;
    let style = |src: &ActiveMapSources| -> String {
        active_map_from_sources(src)
            .get("widgetstyle")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    // KDE — as before.
    let kde = ActiveMapSources { widget_style: "Breeze", ..base() };
    assert_eq!(style(&kde), "Breeze");

    // The session variable takes precedence: Qt reads it before the platform theme.
    let overridden = ActiveMapSources {
        widget_style: "Breeze",
        qt_style_override_env: "kvantum",
        ..base()
    };
    assert_eq!(style(&overridden), "kvantum");

    // Our environment.d file: the session has not seen it yet, but the choice is already made.
    let pending = ActiveMapSources {
        qt_style_env_file: Some("# Rexon\nQT_STYLE_OVERRIDE=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&pending), "kvantum");

    // LXQt — its own lxqt.conf, section [Qt].
    let lxqt = ActiveMapSources {
        lxqt_conf: Some("[General]\ntheme=frost\n\n[Qt]\nstyle=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&lxqt), "kvantum");

    // qt6ct — section [Appearance]; a key from SOMEONE ELSE'S section is not taken.
    let qtct = ActiveMapSources {
        qtct_conf: Some("[Fonts]\nstyle=Fusion\n[Appearance]\nstyle=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&qtct), "kvantum");

    // Nothing set — no key at all (as before).
    assert_eq!(style(&base()), "");
}

/// In English the card labels are the source ones: the theme kind and the active badge.
#[test]
fn card_labels_in_english() {
    let _lang = i18n_pin::en();
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: "en",
        standard_icons: None,
    };
    let active = scan_active(&env, &e.amap_full);
    let colors = active
        .iter()
        .find(|it| it.id.starts_with("local:colors:"))
        .expect("активная цветовая схема есть в снимке");
    assert_eq!(colors.author, "🌈 Color Scheme · ✅ Active");
    let splash = scan_lnf_component(&env, "splash", &e.amap_full);
    let on = splash.iter().find(|it| it.local_kind.is_some()).unwrap();
    assert_eq!(on.author, "🌅 ✅ Active");
    let off = splash.iter().find(|it| it.local_kind.is_none()).unwrap();
    assert_eq!(off.author, "🌅 KDE Splash Screen");
    let downloaded = scan_downloaded(&env, &e.amap_full);
    assert!(
        downloaded.iter().all(|it| it.author.ends_with(" · ⬇️ Downloaded")),
        "{:?}",
        downloaded.iter().map(|it| &it.author).collect::<Vec<_>>()
    );
}

/// ⭐ Defect of 28.09.2026: with Plasma Login Manager there is no active SDDM theme —
/// even if `Current=breeze` from the former SDDM remained in `/etc/sddm.conf.d`
/// (the owner's live machine). And `Current=` counts only in `[Theme]`.
#[test]
fn sddm_current_only_under_sddm_and_only_in_theme_group() {
    use rexon_core::installed_scan::sddm_confs_for_active_map;
    let _lang = i18n_pin::ru();
    let sddm_of = |confs: Vec<&str>| -> Option<String> {
        let src = ActiveMapSources { sddm_confs: confs, ..ActiveMapSources::default() };
        active_map_from_sources(&src)
            .get("sddm")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    // The file that lies on the owner's machine after switching to PLM.
    let leftover = "[Autologin]\nRelogin=false\n\n[Theme]\nCurrent=breeze\nCursorTheme=breeze_cursors\n";

    // SDDM is active — the theme is marked, as before.
    assert_eq!(sddm_of(sddm_confs_for_active_map(vec![leftover], false)).as_deref(), Some("breeze"));
    // Plasma Login Manager — no mark at all.
    assert_eq!(sddm_of(sddm_confs_for_active_map(vec![leftover], true)), None);

    // `Current=` outside `[Theme]` is not a theme.
    assert_eq!(sddm_of(vec!["[General]\nCurrent=foo\n"]), None);
    assert_eq!(sddm_of(vec!["Current=bare\n"]), None, "ключ до первой группы");
    assert_eq!(
        sddm_of(vec!["[Theme]\nCurrent=a\n[Other]\nCurrent=b\n"]).as_deref(),
        Some("a")
    );
    // The last file wins (the order of sources did not change).
    assert_eq!(
        sddm_of(vec!["[Theme]\nCurrent=elarun\n", "[Theme]\nCurrent=breeze\n"]).as_deref(),
        Some("breeze")
    );
}

/// The «None» tile of the «Splash screen» section (as in the KDE module): last, without
/// a directory (hence without «Remove»), current only when the splash screen
/// is off; the lock screen has no such tile.
#[test]
fn splash_none_tile_is_last() {
    let e = load_env();
    let preview_file = |url: &str| e.preview_files.get(url).cloned();
    let env = ScanEnv {
        home: "/sb/home",
        usr_root: "/sb/usr",
        fs: &e.fs,
        ocs_cache: &e.ocs_cache,
        preview_file: &preview_file,
        store: rexon_core::installed_scan::StoreRecords::none(),
        locale: "en",
        standard_icons: None,
    };
    let splash = scan_lnf_component(&env, "splash", &e.amap_full);
    let none = splash.last().expect("плитки есть");
    assert_eq!(none.id, "local:splash:None");
    assert_eq!(none.install_path, "", "каталога нет — удалять нечего");
    assert_eq!(none.preview_local, "");
    assert!(none.local_kind.is_none(), "заставка включена — «Нет» не текущая");
    assert_eq!(splash.iter().filter(|it| it.local_kind.is_some()).count(), 1);

    let mut off = e.amap_full.clone();
    off.insert("splash".to_string(), "None".to_string());
    let splash = scan_lnf_component(&env, "splash", &off);
    let current: Vec<_> = splash.iter().filter(|it| it.local_kind.is_some()).map(|it| it.id.as_str()).collect();
    assert_eq!(current, ["local:splash:None"]);

    let lock = scan_lnf_component(&env, "lockscreen", &e.amap_full);
    assert!(lock.iter().all(|it| !it.id.ends_with(":None")));
}
