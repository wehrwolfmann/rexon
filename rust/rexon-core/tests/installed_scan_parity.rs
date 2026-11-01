//! Golden-сверка порта `installed_scan.rs` с оригиналом
//! `rexon/appearance/installed_scan.py`.
//!
//! Эталоны сняты с самого Python по схеме «песочница + снимок»:
//!   1. Одноразовый генератор снял снимок РЕАЛЬНОГО дерева тем машины
//!      Wehrwolfmann (~/.local/share/plasma, /usr/share/themes, иконки,
//!      курсоры, Kvantum, sddm, plymouth — только структура + файлы
//!      метаданных, бинарные превью пустышками; пути нормализованы в
//!      /sb/home и /sb/usr) и добавил синтетику: junk look-and-feel без
//!      metadata, битый metadata.json, тема с пустым Name, metadata.desktop
//!      с Name и без, курсорная тема в icons/, цвет-схема с пробелом в
//!      имени, сайдкар-превью, кейсы бэкофилла из кеша магазина
//!      (однозначный и неоднозначный матч).
//!   2. Снимок материализован в РЕАЛЬНУЮ песочницу — Python-эталон гонялся
//!      по настоящим os.scandir/open (файловый слой не мокался), а
//!      `snapshot` в фикстуре хранит порядок имён так, как их вернул
//!      os.scandir эталону → Rust итерирует в том же порядке.
//!   3. Немокаемое в Python: `_active_map`/`_qstyle_factory`/pacman/кеш
//!      магазина подменялись фикстурами — в Rust это параметры сканеров.
//!
//! ⚠️ Снимок УСЕЧЁН по ширине (в каждом корне — полтора десятка тем из
//! реальных сотен), но по глубине честен: тексты `metadata.json`,
//! `metadata.desktop`, `index.theme` и `*.colors` лежат в нём ДОСЛОВНО
//! такими, как на машине Wehrwolfmann. Именно из них берётся подпись
//! карточки, поэтому пустой текст означал бы «у темы нет имени» и голден
//! перестал бы что-либо доказывать про иконки, курсоры, GTK-темы и
//! цветовые схемы.
//!
//! `golden/installed_scan_tree.json` — снимок + эталоны сканеров
//! (dataclasses.asdict ПОЛЕ-В-ПОЛЕ, включая порядок элементов).
//! `golden/installed_scan_pure.json` — чистые парсеры: нормализация имён,
//! сайдкар-ключи, политика `pacman -Qm` (ретраи по db.lck), разбор
//! `pacman -Qo`, сборка `_active_map` из готовых источников (Python гонялся
//! с замоканными ThemeManager/kreadconfig6/конфигами).

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

/// Язык, на котором сняты эталоны: интерфейс у нас русский, и подписи тем
/// в голдене — те, что увидит покупатель. Прибит к фикстуре намеренно —
/// иначе списки зависели бы от локали машины, где гоняются тесты.
const FX_LOCALE: &str = "ru_RU.UTF-8";

// Подписи карточек (вид темы, «✅ Активна», «⬇ Скачано») переводятся, а
// эталоны сняты с русского интерфейса: сверки идут на русском каталоге
// `po/ru.po` и заодно держат слово, что по-русски всё как было.
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

/// Окружение из фикстуры. Возвращает замыкание preview_file отдельно,
/// чтобы времена жизни ссылок в ScanEnv были корректны.
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
// Сканеры по снимку
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
    // Последняя плитка заставки — «Нет» (её в эталоне нет: подпись
    // переводится и сверяется отдельно, `splash_none_tile_is_last`).
    let mut splash = scan_lnf_component(&env, "splash", &e.amap_full);
    assert_eq!(splash.pop().map(|it| it.id), Some("local:splash:None".to_string()));
    assert_eq!(items_json(&splash), e.cases["scan_lnf_splash"], "scan_lnf(splash)");
    assert_eq!(
        items_json(&scan_lnf_component(&env, "lockscreen", &e.amap_full)),
        e.cases["scan_lnf_lockscreen"],
        "scan_lnf(lockscreen)"
    );
}

/// Человеческое имя видно ТОЛЬКО покупателю: номер карточки, путь установки
/// и признак активной темы остались на именах каталогов.
///
/// Здесь же сверяется, что поиск по списку продолжает работать — он
/// фильтрует по тому, что человек читает на карточке (`local_matches`), и
/// после подмены подписи должен находить тему по её НАСТОЯЩЕМУ названию.
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

    // Активная тема помечается по имени каталога, а не по подписи.
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

    // Имя для применения по-прежнему достаётся из номера карточки.
    assert_eq!(
        rexon_core::appearance_page::local_theme_name(&ours.id, &ours.name),
        "org.rexon.sessionsplash"
    );

    // Поиск идёт по подписи — покупатель ищет то, что видит.
    assert!(rexon_core::appearance_page::local_matches(&ours.name, "session"));
    let found = rexon_core::appearance_page::local_filter(&splash, "rexon session");
    assert_eq!(found.len(), 1, "поиск по человеческому имени: {found:?}");
    assert_eq!(found[0].id, ours.id);
}

/// Иконки, курсоры, GTK-темы и цветовые схемы тоже подписаны по-человечески.
///
/// Метаданных KDE у них нет: имя лежит в `index.theme` (иконки, курсоры,
/// GTK) либо внутри самого файла `*.colors`. Проверяется ровно то, что и у
/// look-and-feel: подпись — человеческая, а номер карточки, путь установки
/// и имя для применения остались на имени каталога/файла.
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
    // Тема GTK с переведённым именем: в эталонной карте владельцев её нет.
    owners.insert("/sb/usr/share/themes/BlueMenta".to_string(), "mate-themes".to_string());
    let scan = scan_system(&env, &e.amap_full, true, Some(&foreign), &owners)
        .expect("scan_system ok");

    // вид → (номер карточки, ожидаемая подпись, путь установки)
    let want = [
        // курсор: имя каталога и имя темы РАЗНЫЕ, да ещё и переведены
        ("local:cursor:breeze_cursors", "Breeze, тёмный вариант",
         "/sb/usr/share/icons/breeze_cursors"),
        ("local:cursor:Oxygen_Black", "Oxygen, чёрный вариант",
         "/sb/usr/share/icons/Oxygen_Black"),
        // иконки: имя не переводится, но пишется иначе, чем каталог
        ("local:icons:Cosmic", "COSMIC", "/sb/usr/share/icons/Cosmic"),
        // GTK-тема: имя в [Desktop Entry] index.theme
        ("local:gtk:BlueMenta", "Menta (Синяя)",
         "/sb/usr/share/themes/BlueMenta"),
        // цветовая схема: каталога нет вовсе, имя внутри файла *.colors
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
        // Имя для применения темы по-прежнему достаётся из номера карточки,
        // а не из подписи: иначе plasma-apply-* получил бы «COSMIC».
        let dir = id.rsplit(':').next().unwrap();
        assert_eq!(
            rexon_core::appearance_page::local_theme_name(&it.id, &it.name),
            dir,
            "имя для применения у {id}"
        );
        // Поиск идёт по подписи — покупатель ищет то, что видит.
        assert!(
            rexon_core::appearance_page::local_matches(&it.name, display),
            "поиск по подписи {display:?}"
        );
    }

    // …и по тому, как тему зовут в оригинале: имя без перевода (`Name=`) и
    // имя каталога. Живая проверка r1434: «Adwaita» и «HighContrast» не
    // находились, потому что на карточке стоят «Адвайта» и
    // «Высококонтрастная».
    let by_id = |id: &str| scan.items.iter().find(|it| it.id == id).unwrap().clone();
    // вид → (номер карточки, имя без перевода, запросы, которые обязаны найти)
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
    // Имя без перевода, совпадающее с подписью, не дублируется.
    assert_eq!(by_id("local:colors:IridescentLightly").source_name, "");
    // Оригинальные имена не размывают поиск: чужая тема по ним не находится.
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

    // pacman нет → пусто (вне Arch классификация невозможна).
    let no_pacman = scan_system(&env, &e.amap_full, false, Some(&foreign), &owners)
        .expect("без pacman — Ok");
    assert_eq!(
        items_json(&no_pacman.items),
        e.cases["scan_system_no_pacman"],
        "без pacman"
    );

    // pacman -Qm недоступен → SystemScanUnavailable (текст 1-в-1).
    let err = scan_system(&env, &e.amap_full, true, None, &owners)
        .expect_err("должен быть Err");
    assert_eq!(
        json!(err),
        e.cases["scan_system_unavailable_message"],
        "текст SystemScanUnavailable"
    );
}

// ============================================================================
// Чистые парсеры
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
    // Бэкофилл проверяется и внутри сканеров (WhiteSur-Dark в downloaded);
    // здесь — точечные случаи однозначности.
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
    // Однозначный матч → локальный файл из кеша (эталоны сняты с живого
    // Python: _backfill_preview на этом же кеше).
    assert_eq!(
        backfill_preview(&env, "WhiteSur-Dark"),
        "/sb/home/.cache/rexon/previews/whitesur.png"
    );
    // «nord» ⊂ «nordsomething», а «nordic» — нет → матч ОДНОЗНАЧЕН (только
    // nord-a) и превью подставляется. Python подтверждён: тот же путь.
    assert_eq!(
        backfill_preview(&env, "Nord-something"),
        "/sb/home/.cache/rexon/previews/whitesur.png"
    );
    // Неоднозначный: «nordic» матчит и «Nord» (nord-a), и «Nordic Theme»
    // (nord-b) → 2 URL → пусто (Python подтверждён).
    assert_eq!(backfill_preview(&env, "Nordic"), "");
    // Короткий стержень (<4) → пусто.
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
    // Вне Arch — пустое множество без попыток.
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

/// Живой ответ этой машины, пустой ответ, ответ не выполнившейся команды и
/// мусор в выводе — на одном и том же разборе.
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

/// Команда на весь проект ОДНА: argv, записанный в голдене скана рядом с
/// ответом (`calls`), обязан совпасть с тем, что отдаёт движок пакетов.
/// Разойдётся — значит вторая копия команды снова где-то завелась.
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
            // `library` эталон Python не читал: None — прежнее правило по `theme=`.
            kwin_decoration_library: None,
            kvantum_kvconfig: opt("kvantum_kvconfig"),
            sddm_confs: confs("sddm_confs"),
            plymouth_confs: confs("plymouth_confs"),
            // Источники стиля Qt вне KDE эталон Python не знал вовсе (там он
            // читал только kdeglobals): пустые — значит поведение прежнее.
            qt_style_override_env: s("qt_style_override_env"),
            qt_style_env_file: opt("qt_style_env_file"),
            lxqt_conf: opt("lxqt_conf"),
            qtct_conf: opt("qtct_conf"),
            // Родное хранилище окружения эталон Python тоже не знал: пусто —
            // прежняя цепочка kdeglobals/settings.ini.
            native_gtk: None,
            native_icons: None,
            native_cursor: None,
            // Звук и обои эталон Python тоже не знал: в старых случаях этих
            // источников нет, и карта обязана остаться прежней.
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

/// Звук и обои в карте активных тем: два новых вида считаются той же картой,
/// что и все остальные, — отдельного пути для них нет.
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

    // Звук: ответ команды приходит как есть, кавычки gsettings снимаются.
    let snd = ActiveMapSources { sound_theme: Some("'zorinos2'\n"), ..base() };
    assert_eq!(key(&snd, "sounds"), "zorinos2");
    // Читать нечем — строки в карте нет вовсе (а не пустое значение).
    assert!(active_map_from_sources(&base()).get("sounds").is_none());
    // Тема не задана (ответ пуст) — применённой звуковой темы нет. Прежде
    // пустой ответ подменялся на `freedesktop`, и во вкладке «Оформление»
    // невыбранная тема светилась применённой.
    let empty = ActiveMapSources { sound_theme: Some("  \n"), ..base() };
    assert!(active_map_from_sources(&empty).get("sounds").is_none());

    // Обои: в карту идёт имя ПАЧКИ, внутри которой лежит текущая картинка.
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
    // Одиночная картинка лежит прямо в каталоге обоев — имя пачки = её имя,
    // ровно так она записана и в журнале.
    assert_eq!(wp("/sb/home/.local/share/wallpapers/Горы.png"), "Горы.png");
    // Системная пачка.
    assert_eq!(wp("/sb/usr/share/wallpapers/Next/"), "Next");
    // gsettings отдаёт URI с процентами — раскодируем, иначе кириллица не
    // совпадёт ни с одной пачкой.
    assert_eq!(
        wp("'file:///sb/home/.local/share/wallpapers/%D0%9F%D0%B0%D1%87%D0%BA%D0%B0/a.png'"),
        "Пачка"
    );
    // Свой файл вне каталогов обоев пачкой не считается.
    assert_eq!(wp("/sb/home/Изображения/своё.png"), "");
}

/// Бейдж «Активна» у темы Kvantum должен работать не только в KDE.
///
/// Активность определяется по widgetStyle, а он вне KDE берётся не из
/// kdeglobals: переменная сеанса, наш файл environment.d, lxqt.conf, конфиг
/// qt6ct. Пока источник был один, вне KDE бейдж не появлялся никогда.
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

    // KDE — как было.
    let kde = ActiveMapSources { widget_style: "Breeze", ..base() };
    assert_eq!(style(&kde), "Breeze");

    // Переменная сеанса главнее: Qt читает её раньше платформенной темы.
    let overridden = ActiveMapSources {
        widget_style: "Breeze",
        qt_style_override_env: "kvantum",
        ..base()
    };
    assert_eq!(style(&overridden), "kvantum");

    // Наш файл environment.d: сеанс его ещё не видел, но выбор уже сделан.
    let pending = ActiveMapSources {
        qt_style_env_file: Some("# Rexon\nQT_STYLE_OVERRIDE=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&pending), "kvantum");

    // LXQt — свой lxqt.conf, секция [Qt].
    let lxqt = ActiveMapSources {
        lxqt_conf: Some("[General]\ntheme=frost\n\n[Qt]\nstyle=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&lxqt), "kvantum");

    // qt6ct — секция [Appearance]; ключ из ЧУЖОЙ секции не берём.
    let qtct = ActiveMapSources {
        qtct_conf: Some("[Fonts]\nstyle=Fusion\n[Appearance]\nstyle=kvantum\n"),
        ..base()
    };
    assert_eq!(style(&qtct), "kvantum");

    // Ничего не задано — ключа нет вовсе (как и раньше).
    assert_eq!(style(&base()), "");
}

/// По-английски подписи карточек — исходные: вид темы и бейдж активной.
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

/// ⭐ Дефект 28.09.2026: при Plasma Login Manager активной SDDM-темы нет —
/// даже если в `/etc/sddm.conf.d` остался `Current=breeze` от прежнего SDDM
/// (живая машина владельца). И `Current=` засчитывается только в `[Theme]`.
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
    // Файл, что лежит на машине владельца после перехода на PLM.
    let leftover = "[Autologin]\nRelogin=false\n\n[Theme]\nCurrent=breeze\nCursorTheme=breeze_cursors\n";

    // SDDM активен — тема отмечается, как и прежде.
    assert_eq!(sddm_of(sddm_confs_for_active_map(vec![leftover], false)).as_deref(), Some("breeze"));
    // Plasma Login Manager — отметки нет вовсе.
    assert_eq!(sddm_of(sddm_confs_for_active_map(vec![leftover], true)), None);

    // `Current=` вне `[Theme]` — не тема.
    assert_eq!(sddm_of(vec!["[General]\nCurrent=foo\n"]), None);
    assert_eq!(sddm_of(vec!["Current=bare\n"]), None, "ключ до первой группы");
    assert_eq!(
        sddm_of(vec!["[Theme]\nCurrent=a\n[Other]\nCurrent=b\n"]).as_deref(),
        Some("a")
    );
    // Последний файл выигрывает (порядок источников не изменился).
    assert_eq!(
        sddm_of(vec!["[Theme]\nCurrent=elarun\n", "[Theme]\nCurrent=breeze\n"]).as_deref(),
        Some("breeze")
    );
}

/// Плитка «Нет» раздела «Заставка» (как у модуля KDE): последняя, без
/// каталога (значит, без «Удалить»), текущая — только когда заставка
/// выключена; у экрана блокировки такой плитки нет.
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
