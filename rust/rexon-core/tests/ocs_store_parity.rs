//! Golden-сверка порта `ocs_store.rs` с оригиналом `rexon/ocs_store.py`.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.ocs_store`: под детект типа темы
//! во временном каталоге строились НАСТОЯЩИЕ деревья файлов (цветовая
//! схема, Aurorae, Kvantum, иконки с подпапками и без, «полный пак» из
//! двух компонентов и из одного, `contents/defaults`, `contents/widgets`,
//! три варианта `metadata.desktop`, пустой каталог, отсутствующий путь) —
//! и рядом снимался снимок наблюдений, который получает порт. Так
//! сверяется решение, а не переписанный обход файловой системы.
//!
//! Журнал установленного снимался в отдельном временном HOME с
//! замороженным временем; реальный `~/.config/rexon` не трогался.
//!
//! `golden/ocs_store_cases.json` — снятые значения.

use rexon_core::ocs_store as store;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/ocs_store_cases.json")).unwrap()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

fn scan_from(v: &Value) -> store::KdeContentScan {
    store::KdeContentScan {
        exists: v["exists"].as_bool().unwrap(),
        files: strs(&v["files"]),
        subdirs: strs(&v["subdirs"]),
        has_contents_dir: v["has_contents_dir"].as_bool().unwrap(),
        has_contents_defaults: v["has_contents_defaults"].as_bool().unwrap(),
        contents_subdirs: strs(&v["contents_subdirs"]),
        metadata_text: v["metadata_text"].as_str().map(str::to_string),
    }
}

#[test]
fn detect_matches_original_on_real_trees() {
    let g = fx();
    let want = g["detect"].as_object().unwrap();
    let inputs = g["detect_input"].as_object().unwrap();
    assert_eq!(want.len(), inputs.len(), "наборы кейсов разошлись");

    for (case, expected) in want {
        let scan = scan_from(&inputs[case]);
        let got = store::detect_actual_kde_type(&scan);
        let expected = expected.as_str();
        assert_eq!(got, expected, "кейс {case}");
    }
}

/// Порядок проверок значим: тема, подходящая сразу под два признака,
/// должна решаться так же, как в оригинале.
#[test]
fn detect_priority_is_preserved() {
    let g = fx();
    let inputs = g["detect_input"].as_object().unwrap();

    // Цветовая схема побеждает Aurorae…
    let scan = scan_from(&inputs["priority_colors_over_aurorae"]);
    assert_eq!(store::detect_actual_kde_type(&scan), Some("color-scheme"));
    // …а «полный пак» побеждает цветовую схему.
    let scan = scan_from(&inputs["priority_bundle_over_colors"]);
    assert_eq!(store::detect_actual_kde_type(&scan), Some("bundle"));
    // Один компонент паком не считается.
    let scan = scan_from(&inputs["bundle_one"]);
    assert_eq!(store::detect_actual_kde_type(&scan), None);
    // index.theme без характерных подпапок иконками не считается.
    let scan = scan_from(&inputs["icons_no_sub"]);
    assert_eq!(store::detect_actual_kde_type(&scan), None);
}

#[test]
fn install_log_paths() {
    let path = store::install_log_path("/home/u");
    assert!(
        path.ends_with(fx()["log_path_tail"].as_str().unwrap()),
        "путь журнала: {path}"
    );
    assert_eq!(store::install_log_tmp_path("/x/y.json"), "/x/y.json.tmp");
}

#[test]
fn record_and_serialize_byte_for_byte() {
    let g = fx();
    let item = store::InstalledItem {
        content_id: "1687249".into(),
        name: "Dracula GTK".into(),
        rexon_type: "theme-gtk".into(),
        author: "Автор".into(),
        description: "Опис<ание>".into(),
        summary: "кратко".into(),
        version: "1.2".into(),
        license: "GPL".into(),
        preview_url: "http://u/п.png".into(),
        score: 87,
        downloads: 12345,
        tags: vec!["dark".into(), "тёмная".into()],
        web_url: "http://w".into(),
        provider_id: "gnome-look".into(),
        category_id: 135,
    };
    let mut log = store::parse_installed_log(None);
    assert!(log.is_empty(), "пустой журнал без файла");

    store::record_installed(
        &mut log,
        &item,
        // Пустой путь обязан отфильтроваться — он приходил от неудавшихся
        // шагов установки.
        &["/usr/share/themes/Dracula".to_string(), String::new()],
        "2026-07-26T15:00:00",
    );
    assert_eq!(
        store::serialize_installed_log(&log),
        g["log_text"].as_str().unwrap(),
        "текст журнала"
    );

    // Разбор своего же вывода даёт то, что вернул оригинал.
    let parsed = store::parse_installed_log(Some(g["log_text"].as_str().unwrap()));
    assert_eq!(Value::Object(parsed), g["log_parsed"]);
}

#[test]
fn forget_removes_only_existing() {
    let g = fx();
    let mut log = store::parse_installed_log(Some(g["log_text"].as_str().unwrap()));

    assert_eq!(
        store::forget_installed(&mut log, "нет"),
        g["forget_miss"].as_bool().unwrap(),
        "удаление отсутствующей записи"
    );
    assert_eq!(
        store::forget_installed(&mut log, "1687249"),
        g["forget_hit"].as_bool().unwrap(),
        "удаление существующей записи"
    );
    assert_eq!(
        store::serialize_installed_log(&log),
        g["log_after_forget"].as_str().unwrap(),
        "журнал после удаления"
    );
}

/// Шаг 5б слияния установщиков: писатели пишут общий журнал, а
/// `ocs_installed.json` получает его зеркало — тот же текст, что писала
/// прежняя логика, на тех же эталонах (запись, затем удаление).
#[test]
fn journal_mirror_matches_old_log_text() {
    let g = fx();
    let dir = std::env::temp_dir().join(format!("rexon-ocs-mirror-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let log = dir.join("rexon/ocs_installed.json").to_string_lossy().into_owned();
    let journal = rexon_core::install_journal::journal_path(&dir.join("rexon"));

    let parsed = store::parse_installed_log(Some(g["log_text"].as_str().unwrap()));
    rexon_core::install_journal::put_store_log(&log, &parsed).expect("журнал записан");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), g["log_text"].as_str().unwrap(), "зеркало после записи");
    assert!(journal.is_file(), "общий журнал не записан");

    let mut data = rexon_core::install_journal::store_log_view(&log);
    assert_eq!(Value::Object(data.clone()), g["log_parsed"], "читатель видит не то");
    assert!(store::forget_installed(&mut data, "1687249"));
    rexon_core::install_journal::put_store_log(&log, &data).expect("журнал записан");
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        g["log_after_forget"].as_str().unwrap(),
        "зеркало после удаления"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Битый или чужой файл журнала не должен ронять карточки установленного.
#[test]
fn broken_log_degrades_to_empty() {
    for text in ["", "не json", "[1,2,3]", "\"строка\"", "null", "42"] {
        assert!(
            store::parse_installed_log(Some(text)).is_empty(),
            "текст {text:?} должен давать пустой журнал"
        );
    }
    assert_eq!(store::serialize_installed_log(&Default::default()), "{}");
}
