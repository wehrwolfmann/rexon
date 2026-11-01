//! Перехватчик паники Rust-окна в настоящем процессе.
//!
//! Тест запускает сам себя дочерним процессом (фильтр `--exact
//! panic_child`) под приватными HOME и XDG_RUNTIME_DIR в $HOME (не в /tmp) и
//! без окна (`REXON_CRASH_NO_DIALOG=1`); ребёнок ставит перехватчик и
//! паникует дважды в рабочих потоках. Родитель проверяет файл: он один (второй
//! паники в отчётах нет), лежит там же, где отчёты Python-хозяина, в той же
//! раскладке шапки и без недописанных временных файлов.

use std::path::PathBuf;
use std::process::Command;

const CHILD_ENV: &str = "REXON_CRASH_TEST_CHILD";

#[test]
fn panic_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    rexon_gui::crash_report::install();
    let first = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("проверочная паника: рабочий поток"))
        .unwrap()
        .join();
    assert!(first.is_err());
    let second = std::thread::spawn(|| panic!("вторая паника")).join();
    assert!(second.is_err());
}

fn private_home() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from(std::env::var("HOME").unwrap())
        .join(".cache")
        .join(format!("rexon-crash-hook-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

#[test]
fn hook_writes_one_report_like_python() {
    let home = private_home();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "panic_child", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("HOME", &home)
        .env_remove("XDG_CONFIG_HOME")
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env(rexon_gui::crash_report::NO_DIALOG_ENV, "1")
        .env("LANG", "ru_RU.UTF-8")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "ребёнок упал: {stderr}");

    let dir = home.join(".config/rexon/crashes");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("нет каталога отчётов {dir:?}: {e}\n{stderr}"))
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "ровно один отчёт, без временных: {names:?}");
    let name = &names[0];
    assert!(name.starts_with("crash_") && name.ends_with(".log"), "{name}");
    // С 504c97e о крахе говорит строка журнала (rexon-core::applog), её
    // копия уходит в исходный поток ошибок: «… ERROR crash: паника … ;
    // отчёт: <путь>».
    assert!(stderr.contains(&format!("; отчёт: {}/{name}", dir.display())), "{stderr}");

    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Rexon crash report");
    assert!(lines[1].starts_with("Time:     2"), "{}", lines[1]);
    assert!(lines[2].starts_with("Rexon:    0."), "{}", lines[2]);
    assert!(lines[3].starts_with("Qt:       6."), "{}", lines[3]);
    assert!(lines[4].starts_with("OS:       Linux-"), "{}", lines[4]);
    assert!(lines[5].starts_with("DE:       "), "{}", lines[5]);
    assert_eq!(lines[6], "Locale:   ru_RU.UTF-8");
    assert_eq!(lines[7], "─".repeat(60));
    assert_eq!(lines[8], "");
    assert_eq!(lines[9], "Rust panic: проверочная паника: рабочий поток");
    assert_eq!(lines[10], "Thread:   worker");
    assert!(lines[11].starts_with("Location: ") && lines[11].contains("crash_report.rs:"), "{}", lines[11]);
    assert!(text.contains("\nBacktrace:\n"), "{text}");
    assert!(!text.contains("вторая паника"), "вторая паника в отчёт не идёт");
    // Имя файла — от того же момента, что строка Time (починка №1 ядра).
    let stamp = &name["crash_".len()..name.len() - 4];
    let time = lines[1].trim_start_matches("Time:     ");
    let expect = format!(
        "{}{}{}_{}{}{}",
        &time[0..4], &time[5..7], &time[8..10], &time[11..13], &time[14..16], &time[17..19]
    );
    assert_eq!(stamp, expect);

    std::fs::remove_dir_all(&home).unwrap();
}
