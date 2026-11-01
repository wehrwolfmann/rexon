//! Golden-сверка порта `backup.rs` с оригиналом `rexon/backup.py`.
//!
//! Эталон `golden/backup_cases.json` снят с самого Python: `os.path.relpath` +
//! логика `create_backup`, `is_plasma_running`, `_expand`, `get_backups_for_de`
//! и list-выборки `PlasmaProtector` прогнаны на батарее входов. Проверяются
//! только детерминированные (чистые) части — эффекты ФС/подпроцессов в порт не
//! входят.

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
    // FULL начинается с PANEL (инвариант оригинала).
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
    // Метаданные из того же набора, что в оракуле (порядок вставки сохранён).
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

    // Пустой набор.
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
//  Раскладки/выборки/сообщения второй волны порта (секция `pure_v2`)
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

    // restore_items: наличие файла в снапшоте берём из списка `present`
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

    // раскладка снимка панели: существование конфигов — из списка `present`
    let s = &p["plasma_snapshot_items"];
    let present = strs(&s["present"]);
    assert_eq!(
        bk::plasma_snapshot_items(&strs(&s["configs"]), s["snap_dir"].as_str().unwrap(), |x| {
            present.iter().any(|q| q == x)
        }),
        pairs(&s["want"])
    );

    // запись о новом снимке
    let want_entry = &p["plasma_entry"];
    let got = bk::plasma_entry(
        want_entry["timestamp"].as_str().unwrap(),
        want_entry["reason"].as_str().unwrap(),
        want_entry["path"].as_str().unwrap(),
        &strs(&want_entry["files"]),
        want_entry["full"].as_bool().unwrap(),
    );
    assert_eq!(serde_json::to_value(&got).unwrap(), *want_entry);

    // ротация: что осталось и что удалять
    let rot = &p["plasma_rotate"];
    let mut list = plasma_vec(&rot["input"]);
    let dropped = bk::plasma_rotate(&mut list);
    assert_eq!(serde_json::to_value(&list).unwrap(), rot["kept"]);
    assert_eq!(dropped, strs(&rot["dropped"]));
    assert_eq!(list.len(), bk::MAX_PLASMA_SNAPSHOTS);

    // Выбор снимка (по метке / последний / ошибки).
    //
    // ⭐Буквы обоих отказов с 18.09 сознательно разошлись с оригиналом
    // (жаргон «бэкап» в лицо покупателю, правило #13) и проверяются в
    // `texts_and_restart_cmds_match_python`. Здесь сверяется РЕШЕНИЕ: тот
    // же ли отказ выбран, что у оригинала, — именно это и есть предмет
    // сверки, а не формулировка.
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

    // источники восстановления панели
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

    // индекс для удаления
    let list = plasma_vec(&p["plasma_delete_backups"]);
    for case in p["plasma_delete_index"].as_array().unwrap() {
        let got = bk::plasma_delete_index(&list, case["timestamp"].as_str().unwrap());
        assert_eq!(json!(got), case["want"]);
    }
}

/// ⭐СОЗНАТЕЛЬНОЕ расхождение с оригиналом, а не подгонка эталона.
///
/// Эталон `golden/backup_cases.json` описывает Python КАК ЕСТЬ и таким и
/// остаётся: его не переписывают под новый текст. Здесь проверяется пара
/// условий сразу — эталон обязан помнить ПРЕЖНЮЮ формулировку (иначе мы
/// чиним не то, что было), а нынешняя обязана от неё отличаться (иначе
/// починка осталась в комментарии и не доехала до кода). Тот же приём, что
/// у `TEXT_FIXES` в `qt_settings_parity.rs`.
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
    // Эталон — русский текст, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = fixture();
    let p = &fx["pure_v2"];
    let t = &p["texts"];
    assert_eq!(json!(bk::MAX_PLASMA_SNAPSHOTS), t["MAX_PLASMA_SNAPSHOTS"]);
    assert_eq!(json!(bk::msg_backup_not_found()), t["BACKUP_NOT_FOUND"]);
    assert_eq!(json!(bk::msg_none_restored()), t["NONE_RESTORED"]);

    // ── Проход 18.09: жаргон в том, что читает покупатель (правило #13) ──
    //
    // Секция «Защита панели KDE Plasma» подписана словом «копия»
    // («Сохранить копию», «Сохранённые копии», «Папка копии»), а сообщения
    // из ядра отвечали на неё «бэкапом» и «снапшотом» — два имени одной
    // вещи в одном окне. Плюс «Plasmashell» вместо того, что человек видит
    // на экране: соседняя строка на неудачу с самого начала говорила
    // «панель».
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

    // Та же статья: счёт стоял с одним окончанием на все числа
    // («Восстановлено 1 элементов», «Бэкап создан (1 файлов)»), а метка
    // времени копии показывалась машинной — при том что строка списка,
    // на которую человек нажал, подписана датой.
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

/// Сторож на будущее: сообщения раздела «Защита панели KDE Plasma» читает
/// покупатель, а не разработчик.
///
/// Проверяются свойства текста, а не его буквы: вторая копия подписи в
/// сверке ровно тем и плоха, что переименование подписи молча перестаёт
/// что-либо значить. Здесь сторож краснеет, если жаргон вернётся любым
/// словом или если счёт снова встанет с одним окончанием.
#[test]
fn panel_messages_speak_to_the_buyer() {
    let _lang = i18n_pin::ru();
    let samples = panel_message_samples();
    // «Plasmashell» — имя служебной программы: на экране человек видит
    // панель. Имена технологий (KDE, Plasma) при этом остаются латиницей —
    // это отдельное правило владельца, и под запрет они не попадают.
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

/// Все сообщения раздела «Защита панели KDE Plasma» на действующем языке.
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

/// Русские счёт и даты — продолжение сверки выше (язык уже закреплён).
fn panel_messages_russian_tail() {
    // Склонение счёта: 1 — «файл», 4 — «файла», 5 — «файлов». Без этого
    // покупатель читал «создана копия (1 файлов)».
    assert!(bk::msg_backup_created(1).ends_with("(1 файл)"));
    assert!(bk::msg_backup_created(4).ends_with("(4 файла)"));
    assert!(bk::msg_backup_created(5).ends_with("(5 файлов)"));
    // Ловушка второго десятка: 11 и 14 — «файлов», а не «файл»/«файла».
    assert!(bk::msg_backup_created(11).ends_with("(11 файлов)"));
    assert!(bk::msg_backup_created(14).ends_with("(14 файлов)"));
    assert!(bk::msg_restored_items(1).ends_with("1 элемент"));
    assert!(bk::msg_restored_items(3).ends_with("3 элемента"));
    assert!(bk::msg_restored_items(11).ends_with("11 элементов"));
    assert!(bk::msg_restored_files(1, "20260730_010203").contains("1 файл из"));

    // Метка времени приводится к тому же виду, каким подписана строка
    // списка копий в окне; неразобранная остаётся как есть, а не
    // превращается в пустое место.
    // Вид даты — из каталога перевода (`qt_settings::pretty_backup_ts`).
    let restored = bk::msg_restored_files(4, "20260730_010203");
    assert!(restored.contains(&rexon_core::qt_settings::pretty_backup_ts("20260730_010203")));
    assert!(restored.contains("30.07.2026") && !restored.contains("20260730_010203"));
    assert!(bk::msg_snapshot_not_found("мусор").contains("мусор"));
}

/// Та же секция по-английски: ни одной русской буквы, счёт по-английски,
/// хвост о перезапуске панели — со своим переводом строки.
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

/// Всё прежнее по-русски — дословно как было до перевода.
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
//  Транзакционное восстановление на РЕАЛЬНЫХ конфигах (секция `tx`)
// ══════════════════════════════════════════════════════════════════════════
//
// Golden снят прогоном Python-эталона `_py_transactional_restore` в tempdir на
// настоящих `~/.config/{kdeglobals,plasmarc,kcminputrc}` этой машины (снимок
// «до» и «после» лежит в JSON). Здесь то же дерево разворачивается заново,
// прогоняется Rust-порт и сверяется побайтово дерево «после» + (ok, message,
// restored). Всё строго внутри временного каталога — в систему тест не пишет.

/// Развернуть описание дерева из golden.
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

/// Снять описание дерева в том же формате, что генератор golden.
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

        // Форсировать сбой atomic swap: rollback-путь — непустой каталог 0555
        // (удалить нельзя → rename(dest → rollback) даёт ENOTEMPTY).
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
            // ⭐Успешный итог считает восстановленные элементы, и с 18.09 он
            // их СКЛОНЯЕТ (оригинал писал «1 элементов» при любом числе —
            // правило #16). Поэтому сверяется само число, а не буквы: их
            // держит `panel_messages_speak_to_the_buyer`.
            assert_eq!(
                out.message,
                bk::msg_transaction_restored(out.restored),
                "{name}: message (оригинал говорил {want_msg:?})"
            );
        } else {
            // хвост сообщения — системная строка ошибки (у Rust и Python своя),
            // сверяем стабильный префикс до двоеточия.
            assert!(
                out.message.starts_with(want_msg),
                "{name}: message {:?} не начинается с {:?}",
                out.message,
                want_msg
            );
        }

        // вернуть права, иначе снять дерево и удалить каталог не получится
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
