//! Golden check of the `ocs_store.rs` port against the original `rexon/ocs_store.py`.
//!
//! The golden was captured read-only from LIVE `rexon.ocs_store`: for theme type detection
//! REAL file trees were built in a temporary directory (a color
//! scheme, Aurorae, Kvantum, icons with and without subfolders, a "full pack" of
//! two components and of one, `contents/defaults`, `contents/widgets`,
//! three variants of `metadata.desktop`, an empty directory, a missing path) —
//! and next to it a snapshot of the observations the port receives was taken. This way
//! the decision is checked, not a rewritten file system walk.
//!
//! The installed log was captured in a separate temporary HOME with
//! frozen time; the real `~/.config/rexon` was not touched.
//!
//! `golden/ocs_store_cases.json` — the captured values.

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

/// The order of checks matters: a theme matching two markers at once
/// must be decided the same way as in the original.
#[test]
fn detect_priority_is_preserved() {
    let g = fx();
    let inputs = g["detect_input"].as_object().unwrap();

    // A color scheme beats Aurorae…
    let scan = scan_from(&inputs["priority_colors_over_aurorae"]);
    assert_eq!(store::detect_actual_kde_type(&scan), Some("color-scheme"));
    // …and a "full pack" beats a color scheme.
    let scan = scan_from(&inputs["priority_bundle_over_colors"]);
    assert_eq!(store::detect_actual_kde_type(&scan), Some("bundle"));
    // One component does not count as a pack.
    let scan = scan_from(&inputs["bundle_one"]);
    assert_eq!(store::detect_actual_kde_type(&scan), None);
    // index.theme without the characteristic subfolders does not count as icons.
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
        // An empty path must be filtered out — it came from failed
        // install steps.
        &["/usr/share/themes/Dracula".to_string(), String::new()],
        "2026-07-26T15:00:00",
    );
    assert_eq!(
        store::serialize_installed_log(&log),
        g["log_text"].as_str().unwrap(),
        "текст журнала"
    );

    // Parsing its own output gives what the original returned.
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

/// Step 5b of merging the installers: the writers write the shared log, and
/// `ocs_installed.json` gets its mirror — the same text the former logic
/// wrote, on the same goldens (write, then removal).
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

/// A broken or foreign log file must not bring down the installed cards.
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
