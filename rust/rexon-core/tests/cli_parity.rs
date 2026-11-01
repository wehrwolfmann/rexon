//! Сторож точки входа: Rust-вход понимает ВСЁ, что понимает продукт.
//!
//! ЗАЧЕМ. Незнакомый аргумент Qt молча игнорирует, поэтому пропущенный флаг
//! не падает и не ругается — он просто открывает обычное окно вместо
//! обещанной работы. Дефект такого вида нельзя увидеть глазами: и с
//! флагом, и без него на экране одно и то же. Значит его обязан ловить
//! сторож, читающий ОБА исходника.
//!
//! ЧТО СТЕРЕЖЁМ. Ожидания — эталон `tests/golden/cli_cases.json`
//! (`include_str!`, то есть пересобирается при любой правке эталона);
//! проверки краснеют сами:
//!
//!   1. каждый служебный флаг эталона либо отработан в
//!      [`rexon_core::cli::run_headless`] с кодом возврата, либо переведён
//!      в раздел окна — молча провалиться в главное окно не может ни один;
//!   2. каталоги переводов самой Qt и разделитель — как в эталоне, иначе
//!      посреди русского окна снова появятся английские кнопки Qt (язык и
//!      откат — язык Rexon, откат на английский);
//!   3. бюджет и ломоть стока фоновых задач — как в эталоне.
//!
//! ПРОИСХОЖДЕНИЕ ЭТАЛОНА. Значения сняты с прежней точки входа на Python
//! (`rexon/app.py`, `rexon/qt_main/app.py`, `rexon/utils.py`); до 02.10.2026
//! сверка читала их замороженные исходники. По решению владельца 02.10.2026
//! Python в Rexon — только архив, и эталон стал собственным договором Rust:
//! правка нашей стороны красит сверку, пока эталон не поправили осознанно.
//!
//! Ни один прогон здесь ничего не запускает и не пишет: маршруты
//! проверяются на разборе аргументов, а не на исполнении.

use rexon_core::{app, cli, exit_guard};

use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/cli_cases.json")).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("массив строк")
        .iter()
        .map(|x| x.as_str().expect("строка").to_string())
        .collect()
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Отработать маршрут «на сухую»: печать собирается, ничего не запускается.
fn dry(flag: &str) -> (Option<i32>, Vec<String>) {
    let mut said: Vec<String> = Vec::new();
    let rc = {
        let mut out = |s: &str| said.push(s.to_string());
        cli::run_headless(&argv(&[flag]), &mut out)
    };
    (rc, said)
}

// ============================================================================
// 1. Ни одна ветка маршрута не проваливается в главное окно молча
// ============================================================================

/// Флаги служебных маршрутов — те, что разбираются ДО Qt. Список из
/// эталона: каждый флаг обязан вести в СВОЙ маршрут, поэтому
/// переименованный у нас флаг красит сторож, а не тихо выпадает из проверки.
fn headless_flags() -> Vec<String> {
    strings(&golden()["headless_flags"])
}

#[test]
fn headless_flags_lead_to_their_own_routes() {
    let flags = headless_flags();
    assert!(!flags.is_empty(), "в эталоне нет служебных флагов");
    let want = [
        ("--apply-defaults", app::Route::ApplyDefaults),
        ("--sanitize-gtk", app::Route::SanitizeGtk),
        ("--session-splash", app::Route::SessionSplash),
    ];
    let got: Vec<&str> = want.iter().map(|(f, _)| *f).collect();
    assert_eq!(flags, got, "служебные флаги разошлись с эталоном");
    for (flag, route) in want {
        assert_eq!(app::route(&argv(&[flag])), route, "флаг {flag}");
    }
    let recover = golden()["recover_flag"].as_str().unwrap().to_string();
    assert_eq!(app::RECOVER_KDE_FLAG, recover, "флаг аварийного отката");
}

/// ⭐ ГЛАВНАЯ ПРОВЕРКА. У служебного флага ОБЯЗАН быть код возврата: Python
/// отдавал 0/1 (`return 0 if ok else 1`), Rust-бинарь до правки не отдавал
/// вовсе — autostart-хук отчитывался нулём при неудаче.
#[test]
fn у_каждого_служебного_флага_есть_код_возврата() {
    for flag in headless_flags() {
        let route = app::route(&argv(&[&flag]));
        assert!(
            !matches!(route, app::Route::MainWindow),
            "флаг {flag} разбирается как главное окно"
        );
    }
    // ⛔ САМ ПРОГОН ЗДЕСЬ НЕ ДЕЛАЕТСЯ НИ ДЛЯ ОДНОГО ИЗ НИХ: у всех четырёх
    // побочные действия на живой машине. `--apply-defaults` применяет
    // дефолты приложений, `--session-splash` запускает mpv,
    // `--sanitize-gtk` правит конфиги GTK, а `--recover-kde` меняет тему,
    // экран входа и обои владельца. Порядок шагов и слова каждого маршрута
    // проверяются на ПОДСТАВНОМ вводе-выводе в сверках его исполнителя
    // (`system_exec_parity.rs`, `appdefaults_exec_parity.rs`), где живой
    // машины нет вовсе.
    assert_eq!(
        app::route(&argv(&["--recover-kde"])),
        app::Route::RecoverKde,
        "--recover-kde обязан оставаться маршрутом с кодом возврата"
    );
    // Прогнать безопасно только маршруты ОКНА: побочных действий у них нет,
    // и кода возврата они не дают — окно поднимает бинарь.
    let (rc, said) = dry("--settings-qt");
    assert_eq!(rc, None, "флаг вкладки открывает окно, а не выходит кодом");
    assert!(said.is_empty(), "и ничего не печатает: {said:?}");
}

/// Пять флагов одиночных вкладок продукта переведены в разделы окна — ни
/// один не открывает раздел по умолчанию.
#[test]
fn каждый_флаг_одиночной_вкладки_открывает_свой_раздел() {
    let ours: Vec<String> = app::SINGLE_TAB_FLAGS.iter().map(|(f, _)| f.to_string()).collect();
    assert_eq!(
        ours,
        strings(&golden()["single_tab_flags"]),
        "флаги одиночных вкладок разошлись с эталоном — потерялся или завёлся лишний"
    );
    for (flag, _module) in app::SINGLE_TAB_FLAGS {
        let got = cli::start_section(&argv(&[flag]));
        let (section, _sub) = got
            .as_ref()
            .unwrap_or_else(|| panic!("флаг {flag} не открывает никакого раздела"));
        assert!(
            rexon_core::qt_main::SECTIONS.contains(&section.as_str()),
            "раздел {section} флага {flag} не из состава окна"
        );
    }
}

/// Имена из ярлыка рабочего стола продолжают переводиться — тем же
/// переводом, что у продукта, а не второй копией таблицы.
#[test]
fn имена_из_ярлыка_переводятся_как_у_продукта() {
    for (page, section, subpage) in app::PAGE_TARGETS {
        assert_eq!(
            cli::start_section(&argv(&[&format!("{}{page}", app::PAGE_FLAG)])),
            Some((section.to_string(), subpage.to_string())),
            "ярлык --page={page}"
        );
    }
}

// ============================================================================
// 2. Русские надписи на кнопках самой Qt
// ============================================================================

/// Каталоги переводов самой Qt — как в эталоне, в том же порядке и без
/// лишних.
#[test]
fn qt_translation_catalogs_match_the_golden() {
    let ours: Vec<String> = cli::QT_TRANSLATION_CATALOGS.iter().map(|c| c.to_string()).collect();
    assert_eq!(ours, strings(&golden()["qt_translation_catalogs"]));
}

/// Разделитель — как в эталоне. Язык отката — НАМЕРЕННО не прежний:
/// Python-оригинал грузил каталог по языку СИСТЕМЫ и откатывался на
/// `qtbase_ru` (весь его интерфейс был русским). У Rexon с переводом язык
/// кнопок Qt — язык самого Rexon, а откат — на английский, то есть никакого
/// каталога.
#[test]
fn separator_matches_the_golden_and_fallback_is_english() {
    assert_eq!(
        cli::QT_TRANSLATION_SEPARATOR,
        golden()["qt_translation_separator"].as_str().unwrap(),
        "разделитель имени каталога разошёлся с эталоном"
    );
    let only_ru = |p: &std::path::Path| p.ends_with("qtbase_ru.qm") || p.ends_with("qt_ru.qm");
    let dir = std::path::Path::new("/qt");
    assert!(
        cli::qt_translation_files("ja", dir, &only_ru).is_empty(),
        "языка у Qt нет — откат обязан быть на английский, а не на русский"
    );
    assert!(cli::qt_translation_files("en", dir, &|_| true).is_empty());
    assert_eq!(cli::qt_translation_files("ru", dir, &only_ru).len(), 2);
}

/// Переводчик обязан быть объектом Qt, а не питоновским: питоновский
/// зовётся из потока загрузки QML и намертво сцепляет потоки через GIL.
/// В Rust-входе питоновского нет по построению — стережём, чтобы шим не
/// подменили обработкой в Rust.
#[test]
fn перевод_кнопок_идёт_каталогом_а_не_колбэком() {
    const SHIM_H: &str = include_str!("../../rexon-gui/cpp/rexon_gui_factory.h");
    const SHIM_CPP: &str = include_str!("../../rexon-gui/cpp/rexon_gui_factory.cpp");
    assert!(
        SHIM_H.contains("rexon_install_qt_translations"),
        "шим переводов Qt потерялся из заголовка"
    );
    assert!(
        SHIM_CPP.contains("installTranslator"),
        "шим больше не включает каталог переводов"
    );
    assert!(
        SHIM_CPP.contains("QLibraryInfo::path"),
        "каталог обязан браться у самой Qt, а не носиться своим"
    );
}

// ============================================================================
// 3. Сток фоновых задач
// ============================================================================

#[test]
fn stop_budget_and_slice_match_the_golden() {
    let g = golden();
    assert_eq!(
        exit_guard::DEFAULT_BUDGET_MS,
        g["stop_budget_ms"].as_u64().unwrap(),
        "бюджет стока разошёлся с эталоном"
    );
    assert_eq!(
        exit_guard::WAIT_SLICE_MS,
        g["stop_wait_slice_ms"].as_u64().unwrap(),
        "ломоть ожидания разошёлся с эталоном"
    );
}

/// Порядок обязателен: сначала ПОПРОСИТЬ (сетевые запросы рвутся на месте),
/// потом ждать. Ждать не попросив — значит досидеть до конца сетевого
/// таймаута, а это и есть «закрытие висло до 30 секунд» у оригинала.
#[test]
fn сток_просит_остановиться_прежде_чем_ждать() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let step = Arc::new(AtomicUsize::new(0));
    let asked = Arc::new(AtomicUsize::new(0));
    let waited = Arc::new(AtomicUsize::new(0));

    let (s, a) = (step.clone(), asked.clone());
    exit_guard::register_stopper(move || {
        a.store(s.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
    });
    let (s, w) = (step.clone(), waited.clone());
    exit_guard::register_waiter(move |_ms| {
        w.store(s.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        0
    });

    exit_guard::stop_background_workers(10, false);
    assert_eq!(asked.load(Ordering::SeqCst), 1, "просьбу дёрнули не первой");
    assert_eq!(waited.load(Ordering::SeqCst), 2, "пул досчитали не последним");
}
