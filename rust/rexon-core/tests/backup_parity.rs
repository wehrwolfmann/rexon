//! Golden check of the `backup.rs` port against the original `rexon/backup.py`.
//!
//! The golden file `golden/backup_cases.json` was captured from Python itself: `os.path.relpath` +
//! the logic of `create_backup`, `is_plasma_running`, `_expand`, `get_backups_for_de`
//! and the list selections of `PlasmaProtector` were run over a battery of inputs. Only
//! the deterministic (pure) parts are checked — FS/subprocess effects are not part of
//! the port.

use rexon_core::backup::{
    backup_name, de_backups_for, expand_tilde, is_plasma_desktop, plasma_backups,
    plasma_has_backups, plasma_latest_backup, posix_relpath, safe_config_name, DeMetaEntry,
    PlasmaBackup, FULL_CONFIGS, PANEL_CONFIGS, ROLLBACK_SUFFIX, STAGE_SUFFIX,
};
use serde_json::{json, Value};

mod i18n_pin;

fn fixture() -> Value {
    serde_json::from_str(include_str!("golden/backup_cases.json")).unwrap()
}

#[test]
fn constants_match_python() {
    let fx = fixture();
    let c = &fx["constants"];
    assert_eq!(json!(STAGE_SUFFIX), c["STAGE_SUFFIX"]);
    assert_eq!(json!(ROLLBACK_SUFFIX), c["ROLLBACK_SUFFIX"]);
    assert_eq!(json!(PANEL_CONFIGS), c["PANEL_CONFIGS"], "PANEL_CONFIGS");
    assert_eq!(json!(FULL_CONFIGS), c["FULL_CONFIGS"], "FULL_CONFIGS");
    // FULL starts with PANEL (an invariant of the original).
    assert_eq!(&FULL_CONFIGS[..PANEL_CONFIGS.len()], PANEL_CONFIGS);
}

#[test]
fn relpath_and_safe_name_match_python() {
    let fx = fixture();
    let home = fx["home"].as_str().unwrap();

    for (path, want) in fx["relpaths"].as_object().unwrap() {
        assert_eq!(
            json!(posix_relpath(path, home)),
            *want,
            "posix_relpath({path})"
        );
    }
    for (path, want) in fx["safe_config_name"].as_object().unwrap() {
        assert_eq!(
            json!(safe_config_name(path, home)),
            *want,
            "safe_config_name({path})"
        );
    }
}

#[test]
fn is_plasma_and_expand_match_python() {
    let fx = fixture();
    for (xdg, want) in fx["is_plasma"].as_object().unwrap() {
        assert_eq!(json!(is_plasma_desktop(xdg)), *want, "is_plasma_desktop({xdg})");
    }
    let home = fx["home"].as_str().unwrap();
    assert_eq!(json!(expand_tilde(PANEL_CONFIGS, home)), fx["expand_panel"]);
}

#[test]
fn backup_name_matches_python() {
    let fx = fixture();
    for (key, want) in fx["backup_name"].as_object().unwrap() {
        let (de, ts) = key.split_once('|').unwrap();
        assert_eq!(json!(backup_name(de, ts)), *want, "backup_name({key})");
    }
}

#[test]
fn de_backups_for_matches_python() {
    let fx = fixture();
    // Metadata from the same set as in the oracle (insertion order preserved).
    let entries = vec![
        DeMetaEntry {
            name: "kde_20260101_100000".into(),
            de_name: "kde".into(),
            timestamp: "20260101_100000".into(),
            files_count: 2,
        },
        DeMetaEntry {
            name: "gnome_20260102_100000".into(),
            de_name: "gnome".into(),
            timestamp: "20260102_100000".into(),
            files_count: 1,
        },
        DeMetaEntry {
            name: "kde_20260103_100000".into(),
            de_name: "kde".into(),
            timestamp: "20260103_100000".into(),
            files_count: 0,
        },
        DeMetaEntry {
            name: "kde_dup_100000".into(),
            de_name: "kde".into(),
            timestamp: "20260101_100000".into(),
            files_count: 3,
        },
    ];
    let want = &fx["de_backups_for"];
    for de in ["kde", "gnome", "xfce"] {
        assert_eq!(
            serde_json::to_value(de_backups_for(&entries, de)).unwrap(),
            want[de],
            "de_backups_for({de})"
        );
    }
}

#[test]
fn plasma_list_selectors_match_python() {
    let fx = fixture();

    let backups: Vec<PlasmaBackup> = vec![
        PlasmaBackup {
            timestamp: "20260101_100000".into(),
            reason: "manual".into(),
            path: "/a".into(),
            files: vec!["f1".into()],
            full: false,
        },
        PlasmaBackup {
            timestamp: "20260102_100000".into(),
            reason: "before_install".into(),
            path: "/b".into(),
            files: vec!["f2".into(), "f3".into()],
            full: true,
        },
    ];
    let pl = &fx["plasma_lists"];
    assert_eq!(
        serde_json::to_value(plasma_backups(&backups)).unwrap(),
        pl["get_backups"],
        "plasma get_backups"
    );
    assert_eq!(
        serde_json::to_value(plasma_latest_backup(&backups)).unwrap(),
        pl["get_latest_backup"],
        "plasma get_latest_backup"
    );
    assert_eq!(json!(plasma_has_backups(&backups)), pl["has_backups"]);

    // An empty set.
    let empty: Vec<PlasmaBackup> = Vec::new();
    let pe = &fx["plasma_empty"];
    assert_eq!(serde_json::to_value(plasma_backups(&empty)).unwrap(), pe["get_backups"]);
    assert_eq!(
        serde_json::to_value(plasma_latest_backup(&empty)).unwrap(),
        pe["get_latest_backup"]
    );
    assert_eq!(json!(plasma_has_backups(&empty)), pe["has_backups"]);
}

// ══════════════════════════════════════════════════════════════════════════
//  Layouts/selections/messages of the port's second wave (section `pure_v2`)
// ══════════════════════════════════════════════════════════════════════════

use rexon_core::backup as bk;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_str().unwrap().to_string(),
                p[1].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn plasma(v: &Value) -> PlasmaBackup {
    serde_json::from_value(v.clone()).unwrap()
}

fn plasma_vec(v: &Value) -> Vec<PlasmaBackup> {
    v.as_array().unwrap().iter().map(plasma).collect()
}

#[test]
fn snapshot_and_restore_layout_match_python() {
    let fx = fixture();
    let p = &fx["pure_v2"];
    let home = p["home"].as_str().unwrap();

    let s = &p["snapshot_items"];
    assert_eq!(
        bk::snapshot_items(&strs(&s["paths"]), s["backup_path"].as_str().unwrap(), home),
        pairs(&s["want"])
    );

    // restore_items: whether a file is in the snapshot is taken from the `present` list
    let r = &p["restore_items"];
    let backup_path = r["backup_path"].as_str().unwrap();
    let present: Vec<String> = strs(&r["present"])
        .into_iter()
        .map(|n| format!("{backup_path}/{n}"))
        .collect();
    assert_eq!(
        bk::restore_items(backup_path, &strs(&r["orig_paths"]), home, |x| present
            .iter()
            .any(|q| q == x)),
        pairs(&r["want"])
    );

    let m = &p["de_meta_entry"];
    assert_eq!(
        bk::de_meta_entry(
            m["de_name"].as_str().unwrap(),
            m["timestamp"].as_str().unwrap(),
            &strs(&m["paths"]),
            m["backup_path"].as_str().unwrap(),
        ),
        m["want"]
    );
}

#[test]
fn plasma_decisions_match_python() {
    let fx = fixture();
    let p = &fx["pure_v2"];

    // panel snapshot layout: existence of configs — from the `present` list
    let s = &p["plasma_snapshot_items"];
    let present = strs(&s["present"]);
    assert_eq!(
        bk::plasma_snapshot_items(&strs(&s["configs"]), s["snap_dir"].as_str().unwrap(), |x| {
            present.iter().any(|q| q == x)
        }),
        pairs(&s["want"])
    );

    // the record of a new snapshot
    let want_entry = &p["plasma_entry"];
    let got = bk::plasma_entry(
        want_entry["timestamp"].as_str().unwrap(),
        want_entry["reason"].as_str().unwrap(),
        want_entry["path"].as_str().unwrap(),
        &strs(&want_entry["files"]),
        want_entry["full"].as_bool().unwrap(),
    );
    assert_eq!(serde_json::to_value(&got).unwrap(), *want_entry);

    // rotation: what is kept and what to delete
    let rot = &p["plasma_rotate"];
    let mut list = plasma_vec(&rot["input"]);
    let dropped = bk::plasma_rotate(&mut list);
    assert_eq!(serde_json::to_value(&list).unwrap(), rot["kept"]);
    assert_eq!(dropped, strs(&rot["dropped"]));
    assert_eq!(list.len(), bk::MAX_PLASMA_SNAPSHOTS);

    // Choosing a snapshot (by label / the latest / errors).
    //
    // ⭐The wording of both refusals deliberately diverged from the original since 18.09
    // (the jargon «бэкап» in the buyer's face, rule #13) and is checked in
    // `texts_and_restart_cmds_match_python`. Here the DECISION is checked: whether
    // the same refusal is chosen as in the original — that is exactly the subject of
    // the check, not the wording.
    for case in p["plasma_pick"].as_array().unwrap() {
        let list = plasma_vec(&case["backups"]);
        let ts = case["timestamp"].as_str();
        match bk::plasma_pick(&list, ts) {
            Ok(entry) => assert_eq!(serde_json::to_value(entry).unwrap(), case["want"]),
            Err(msg) => {
                let want_err = case["want_err"].as_str().unwrap();
                let want_empty = want_err.starts_with("Нет бэкапов");
                let got = if want_empty {
                    bk::msg_no_plasma_backups()
                } else {
                    bk::msg_snapshot_not_found(ts.unwrap_or_default())
                };
                assert_eq!(
                    msg, got,
                    "выбран не тот отказ, что у оригинала ({want_err:?})"
                );
            }
        }
    }

    // panel restore sources
    let ri = &p["plasma_restore_items"];
    let entry = plasma(&ri["entry"]);
    let snap_dir = entry.path.clone();
    let present = strs(&ri["present"]);
    assert_eq!(
        bk::plasma_restore_items(&entry, |x| {
            present.iter().any(|n| format!("{snap_dir}/{n}") == x)
        }),
        pairs(&ri["want"])
    );

    // index for deletion
    let list = plasma_vec(&p["plasma_delete_backups"]);
    for case in p["plasma_delete_index"].as_array().unwrap() {
        let got = bk::plasma_delete_index(&list, case["timestamp"].as_str().unwrap());
        assert_eq!(json!(got), case["want"]);
    }
}

/// ⭐A DELIBERATE divergence from the original, not a fudge of the golden file.
///
/// The golden file `golden/backup_cases.json` describes Python AS IT IS and stays
/// that way: it is not rewritten for the new text. Here a pair of
/// conditions is checked at once — the golden file must remember the FORMER wording (otherwise we
/// are fixing something that was not there), and the current one must differ from it (otherwise
/// the fix stayed in a comment and never reached the code). The same technique as
/// `TEXT_FIXES` in `qt_settings_parity.rs`.
fn diverged(got: &str, golden: &Value, was: &str, what: &str) {
    assert_eq!(
        golden,
        &json!(was),
        "{what}: эталон помнит не ту прежнюю формулировку — проверь, что \
         чинится именно то, что говорил оригинал"
    );
    assert_ne!(got, was, "{what}: починка не применена");
}

#[test]
fn texts_and_restart_cmds_match_python() {
    // The golden value is Russian text, checked through po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let p = &fx["pure_v2"];
    let t = &p["texts"];
    assert_eq!(json!(bk::MAX_PLASMA_SNAPSHOTS), t["MAX_PLASMA_SNAPSHOTS"]);
    assert_eq!(json!(bk::msg_backup_not_found()), t["BACKUP_NOT_FOUND"]);
    assert_eq!(json!(bk::msg_none_restored()), t["NONE_RESTORED"]);

    // ── Pass of 18.09: jargon in what the buyer reads (rule #13) ──
    //
    // The «Защита панели KDE Plasma» section is labelled with the word «копия»
    // («Сохранить копию», «Сохранённые копии», «Папка копии»), while the messages
    // from the core answered it with «бэкап» and «снапшот» — two names for one
    // thing in one window. Plus «Plasmashell» instead of what the person sees
    // on screen: the neighbouring failure line said «панель»
    // from the very beginning.
    diverged(
        &bk::msg_nothing_to_backup(),
        &t["NOTHING_TO_BACKUP"],
        "Нет файлов для бэкапа",
        "NOTHING_TO_BACKUP",
    );
    diverged(
        &bk::msg_no_plasma_backups(),
        &t["NO_PLASMA_BACKUPS"],
        "Нет бэкапов Plasma",
        "NO_PLASMA_BACKUPS",
    );
    diverged(
        &bk::msg_plasmashell_restarted(),
        &t["PLASMASHELL_RESTARTED"],
        "\nPlasmashell перезапущен",
        "PLASMASHELL_RESTARTED",
    );

    let m = &p["messages"];
    assert_eq!(
        json!(bk::msg_restore_failed("swap failed")),
        m["restore_failed"]
    );
    assert_eq!(json!(bk::msg_restored_items(0)), m["restored_items_0"]);

    // The same item: the count had one ending for all numbers
    // («Восстановлено 1 элементов», «Бэкап создан (1 файлов)»), and the copy's
    // timestamp was shown in machine form — even though the list row
    // the person clicked is labelled with a date.
    diverged(
        &bk::msg_restored_items(3),
        &m["restored_items_3"],
        "Восстановлено 3 элементов",
        "restored_items_3",
    );
    diverged(
        &bk::msg_transaction_restored(3),
        &m["transaction_restored_3"],
        "восстановлено 3 элементов",
        "transaction_restored_3",
    );
    diverged(
        &bk::msg_backup_created(5),
        &m["backup_created_5"],
        "Бэкап создан (5 файлов)",
        "backup_created_5",
    );
    diverged(
        &bk::msg_snapshot_not_found("20260730_010203"),
        &m["snapshot_not_found"],
        "Бэкап 20260730_010203 не найден",
        "snapshot_not_found",
    );
    diverged(
        &bk::msg_snapshot_dir_missing("/tmp/bk/nope"),
        &m["snapshot_dir_missing"],
        "Папка бэкапа не существует: /tmp/bk/nope",
        "snapshot_dir_missing",
    );
    diverged(
        &bk::msg_restored_files(4, "20260730_010203"),
        &m["restored_files"],
        "Восстановлено 4 файлов из бэкапа 20260730_010203",
        "restored_files",
    );

    let c = &p["restart_cmds"];
    assert_eq!(bk::plasma_quit_cmd().argv, strs(&c["quit"]));
    assert_eq!(bk::plasma_kill_cmd().argv, strs(&c["kill"]));
    assert_eq!(bk::plasma_start_cmd().argv, strs(&c["start"]));
    assert_eq!(json!(bk::PLASMASHELL_SETTLE_SECS), c["settle_secs"]);
}

/// A guard for the future: the messages of the «Защита панели KDE Plasma» section are read by
/// the buyer, not the developer.
///
/// Properties of the text are checked, not its letters: a second copy of a caption in
/// a check is bad precisely because renaming the caption silently stops
/// meaning anything. Here the guard turns red if the jargon comes back in any
/// word or if the count again gets a single ending.
#[test]
fn panel_messages_speak_to_the_buyer() {
    let _lang = i18n_pin::ru();
    let samples = panel_message_samples();
    // «Plasmashell» is the name of a service program: on screen the person sees
    // the panel. Technology names (KDE, Plasma) stay in Latin script —
    // that is a separate rule of the owner, and they are not banned.
    for (what, text) in &samples {
        let low = text.to_lowercase();
        for jargon in ["бэкап", "снапшот", "plasmashell", "path", "null"] {
            assert!(
                !low.contains(jargon),
                "{what}: покупатель читает жаргон «{jargon}» — {text:?}"
            );
        }
        assert!(
            !text.contains("20260730_010203"),
            "{what}: машинная метка времени в лицо покупателю — {text:?}"
        );
    }
    panel_messages_russian_tail();
}

/// All messages of the «Защита панели KDE Plasma» section in the active language.
fn panel_message_samples() -> Vec<(&'static str, String)> {
    vec![
        ("NOTHING_TO_BACKUP", bk::msg_nothing_to_backup()),
        ("NO_PLASMA_BACKUPS", bk::msg_no_plasma_backups()),
        ("NONE_RESTORED", bk::msg_none_restored()),
        ("BACKUP_NOT_FOUND", bk::msg_backup_not_found()),
        ("PLASMASHELL_RESTARTED", bk::msg_plasmashell_restarted()),
        ("PLASMASHELL_NOT_RESTARTED", bk::msg_plasmashell_not_restarted()),
        ("backup_created", bk::msg_backup_created(3)),
        (
            "snapshot_not_found",
            bk::msg_snapshot_not_found("20260730_010203"),
        ),
        (
            "restored_files",
            bk::msg_restored_files(4, "20260730_010203"),
        ),
        ("restored_items", bk::msg_restored_items(2)),
        ("transaction_restored", bk::msg_transaction_restored(1)),
        ("snapshot_dir_missing", bk::msg_snapshot_dir_missing("/tmp/bk/nope")),
        ("restore_failed", bk::msg_restore_failed("swap failed")),
    ]
}

/// Russian counts and dates — a continuation of the check above (the language is already pinned).
fn panel_messages_russian_tail() {
    // Count declension: 1 — «файл», 4 — «файла», 5 — «файлов». Without it
    // the buyer read «создана копия (1 файлов)».
    assert!(bk::msg_backup_created(1).ends_with("(1 файл)"));
    assert!(bk::msg_backup_created(4).ends_with("(4 файла)"));
    assert!(bk::msg_backup_created(5).ends_with("(5 файлов)"));
    // The second-ten trap: 11 and 14 — «файлов», not «файл»/«файла».
    assert!(bk::msg_backup_created(11).ends_with("(11 файлов)"));
    assert!(bk::msg_backup_created(14).ends_with("(14 файлов)"));
    assert!(bk::msg_restored_items(1).ends_with("1 элемент"));
    assert!(bk::msg_restored_items(3).ends_with("3 элемента"));
    assert!(bk::msg_restored_items(11).ends_with("11 элементов"));
    assert!(bk::msg_restored_files(1, "20260730_010203").contains("1 файл из"));

    // The timestamp is brought to the same form the row of the
    // copies list in the window is labelled with; an unparsed one stays as is rather than
    // turning into an empty space.
    // The date form comes from the translation catalog (`qt_settings::pretty_backup_ts`).
    let restored = bk::msg_restored_files(4, "20260730_010203");
    assert!(restored.contains(&rexon_core::qt_settings::pretty_backup_ts("20260730_010203")));
    assert!(restored.contains("30.07.2026") && !restored.contains("20260730_010203"));
    assert!(bk::msg_snapshot_not_found("мусор").contains("мусор"));
}

/// The same section in English: not a single Russian letter, the count in English,
/// the tail about restarting the panel — with its own line break.
#[test]
fn english_panel_messages_have_no_russian() {
    let _lang = i18n_pin::en();
    for (what, text) in panel_message_samples() {
        assert!(
            !text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
            "{what}: {text:?}"
        );
    }
    assert_eq!(bk::msg_backup_created(1), "Copy saved (1 file)");
    assert_eq!(bk::msg_backup_created(5), "Copy saved (5 files)");
    assert_eq!(bk::msg_restored_items(1), "Restored 1 item");
    assert_eq!(bk::msg_transaction_restored(3), "restored 3 items");
    assert_eq!(bk::msg_plasmashell_restarted(), "\nPanel restarted");
    assert!(bk::msg_plasmashell_not_restarted().starts_with("\nThe panel didn't restart"));
    let restored = bk::msg_restored_files(4, "20260730_010203");
    assert!(restored.starts_with("Restored 4 files from the copy of "), "{restored}");
    assert!(restored.ends_with(&rexon_core::qt_settings::pretty_backup_ts("20260730_010203")));
    assert_eq!(bk::msg_snapshot_not_found("мусор"), "Copy not found: мусор");
}

/// Everything old in Russian — word for word as it was before the translation.
#[test]
fn russian_panel_messages_read_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(bk::msg_backup_not_found(), "Резервная копия не найдена");
    assert_eq!(bk::msg_nothing_to_backup(), "Копировать нечего: файлов настроек панели нет");
    assert_eq!(bk::msg_no_plasma_backups(), "Сохранённых копий панели пока нет");
    assert_eq!(bk::msg_none_restored(), "Не удалось восстановить ни одного файла");
    assert_eq!(bk::msg_plasmashell_restarted(), "\nПанель перезапущена");
    assert_eq!(
        bk::msg_plasmashell_not_restarted(),
        "\nПанель не перезапустилась сама — выйдите из сеанса и войдите снова, \
         чтобы увидеть восстановленную панель"
    );
    assert_eq!(bk::msg_backup_created(2), "Копия сохранена (2 файла)");
    assert_eq!(bk::msg_restored_items(21), "Восстановлено 21 элемент");
    assert_eq!(bk::msg_transaction_restored(5), "восстановлено 5 элементов");
    assert_eq!(bk::msg_snapshot_dir_missing("/x"), "Папка с копией не найдена: /x");
    assert_eq!(bk::msg_restore_failed("boom"), "Восстановление не удалось: boom");
    assert_eq!(
        bk::msg_restored_files(4, "20260730_010203"),
        format!(
            "Восстановлено 4 файла из копии от {}",
            rexon_core::qt_settings::pretty_backup_ts("20260730_010203")
        )
    );
    assert_eq!(bk::msg_snapshot_not_found("мусор"), "Копия не найдена: мусор");
}

// ══════════════════════════════════════════════════════════════════════════
//  Transactional restore on REAL configs (section `tx`)
// ══════════════════════════════════════════════════════════════════════════
//
// The golden file was captured by running the Python reference `_py_transactional_restore` in a tempdir on
// this machine's real `~/.config/{kdeglobals,plasmarc,kcminputrc}` (the "before"
// and "after" snapshot is in the JSON). Here the same tree is unfolded again,
// the Rust port is run and the "after" tree + (ok, message,
// restored) is compared byte for byte. Everything stays strictly inside a temporary directory — the test does not write to the system.

/// Unfold a tree description from the golden file.
fn make_tree(root: &Path, spec: &Value) {
    for (rel, node) in spec.as_object().unwrap() {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        match node["kind"].as_str().unwrap() {
            "dir" => fs::create_dir_all(&p).unwrap(),
            "link" => {
                std::os::unix::fs::symlink(node["target"].as_str().unwrap(), &p).unwrap()
            }
            _ => fs::write(&p, node["text"].as_str().unwrap()).unwrap(),
        }
    }
}

/// Capture a tree description in the same format as the golden generator.
fn snap_tree(root: &Path) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
            let ft = e.file_type().unwrap();
            if ft.is_symlink() {
                out.insert(
                    rel,
                    json!({"kind": "link",
                           "target": fs::read_link(&p).unwrap().to_string_lossy()}),
                );
            } else if ft.is_dir() {
                out.insert(rel, json!({"kind": "dir"}));
                stack.push(p);
            } else {
                out.insert(
                    rel,
                    json!({"kind": "file", "text": fs::read_to_string(&p).unwrap()}),
                );
            }
        }
    }
    out
}

#[test]
fn transactional_restore_matches_python_on_real_configs() {
    let fx = fixture();
    let cases = fx["tx"].as_array().unwrap();
    assert!(!cases.is_empty(), "golden без сценариев транзакции");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let root = std::env::temp_dir().join(format!(
            "rexon-tx-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        make_tree(&root, &case["before"]);

        // Force an atomic swap failure: the rollback path is a non-empty 0555 directory
        // (cannot be deleted → rename(dest → rollback) gives ENOTEMPTY).
        if case["mutate"].as_str() == Some("lock_rollback") {
            let p = root.join(case["mutate_path"].as_str().unwrap());
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join("занято"), "x").unwrap();
            fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o500))
                .unwrap();
        }

        let items: Vec<(String, String)> = pairs(&case["items"])
            .into_iter()
            .map(|(s, d)| {
                (
                    root.join(s).to_string_lossy().to_string(),
                    root.join(d).to_string_lossy().to_string(),
                )
            })
            .collect();
        let out = bk::transactional_restore(&items);

        assert_eq!(json!(out.ok), case["want_ok"], "{name}: ok");
        assert_eq!(
            json!(out.restored),
            case["want_restored"],
            "{name}: restored"
        );
        let want_msg = case["want_message"].as_str().unwrap();
        if out.ok {
            // ⭐The success result counts the restored items, and since 18.09 it
            // DECLINES them (the original wrote «1 элементов» for any number —
            // rule #16). So the number itself is checked, not the letters: they are
            // held by `panel_messages_speak_to_the_buyer`.
            assert_eq!(
                out.message,
                bk::msg_transaction_restored(out.restored),
                "{name}: message (оригинал говорил {want_msg:?})"
            );
        } else {
            // the message tail is the system error string (Rust and Python have their own),
            // we compare the stable prefix up to the colon.
            assert!(
                out.message.starts_with(want_msg),
                "{name}: message {:?} не начинается с {:?}",
                out.message,
                want_msg
            );
        }

        // restore the permissions, otherwise the tree cannot be captured and the directory cannot be removed
        if case["mutate"].as_str() == Some("lock_rollback") {
            let p = root.join(case["mutate_path"].as_str().unwrap());
            let _ = fs::set_permissions(
                &p,
                std::os::unix::fs::PermissionsExt::from_mode(0o700),
            );
        }
        let got = snap_tree(&root);
        let want: BTreeMap<String, Value> =
            serde_json::from_value(case["after"].clone()).unwrap();
        assert_eq!(got, want, "{name}: дерево после восстановления");
        let _ = fs::remove_dir_all(&root);
    }
}
