//! Golden-сверка порта `qt_app.rs` с оригиналом — `rexon/qt_main/app.py` и
//! четырьмя мелкими точками входа.
//!
//! Эталон снят прогоном ЖИВОГО `rexon.qt_main.app`:
//!   * `_crash_header()` — с подменёнными `datetime`/`platform`/`PySide6` и
//!     окружением (`mock.patch.dict`), шесть комбинаций полей;
//!   * `_save_crash_log()` — во ВРЕМЕННОМ каталоге (реальный `~/.config`
//!     не тронут), четыре наполнения каталога;
//!   * `_show_crash_dialog()` — с записывающими заглушками `QApplication`/
//!     `QMessageBox`/`subprocess.Popen`: журнал вызовов окна, буфера и
//!     `xdg-open`;
//!   * `_install_excepthook()` — журнал `sys.excepthook` и
//!     `threading.excepthook` на пяти исключениях;
//!   * снятие пустого `QT_QPA_PLATFORM` — четырьмя запусками интерпретатора,
//!     импортирующего модуль;
//!   * идентичность приложения — разбором исходников двенадцати точек входа
//!     (`ast`), чтобы сверяться с тем, что реально написано в Python.
//!
//! Санкционированные расхождения (починки оригинала, правило #16) проверяются
//! отдельными тестами внизу файла и в голдене НЕ отражены — там записано
//! поведение Python как есть.
//!
//! ГЕЙТ «голден обязан ловить регрессию» проверен мутацией: тринадцать
//! точечных поломок реализации (пустой `XDG_SESSION_TYPE` как отсутствие;
//! `isoformat` всегда с микросекундами; ротация по вхождению `crash_` вместо
//! префикса; ротация на файл длиннее; ротации нет вовсе; пробел вместо
//! перевода строки в подписи лога; `displayName` главного окна не задан;
//! `dirname` возвращает путь целиком; `KeyboardInterrupt` без stderr; hex
//! флагов в верхнем регистре; имя платформы без `repr`; переставленный
//! порядок кнопок; линейка шапки в 59 символов) — все тринадцать красят
//! тесты. Случай `window_hex_letters` заведён именно для этого: на снятых
//! ранее флагах `0x2200001` букв нет, и мутант `{:x}` → `{:X}` был бы
//! эквивалентным, то есть проверка регистра оказалась бы зелёной при любой
//! реализации.
//!
//! `golden/qt_app_cases.json` — снятые значения.

use std::collections::HashMap;

use rexon_core::qt_app::{self, ButtonRole, CrashButton, CrashIo, CrashHeaderInput, Timestamp};
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/qt_app_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn opt_s(v: &Value) -> Option<&str> {
    if v.is_null() {
        None
    } else {
        Some(s(v))
    }
}

fn ts_of(v: &Value) -> Timestamp {
    let a = v.as_array().unwrap();
    Timestamp {
        year: a[0].as_i64().unwrap() as i32,
        month: a[1].as_u64().unwrap() as u32,
        day: a[2].as_u64().unwrap() as u32,
        hour: a[3].as_u64().unwrap() as u32,
        minute: a[4].as_u64().unwrap() as u32,
        second: a[5].as_u64().unwrap() as u32,
        microsecond: a[6].as_u64().unwrap() as u32,
    }
}

// ============================================================================
// Шапка отчёта
// ============================================================================

#[test]
fn crash_header_cases() {
    let g = fx();
    let cases = g["headers"].as_object().unwrap();
    assert_eq!(cases.len(), 7, "все снятые случаи шапки должны сверяться");
    for (name, c) in cases {
        let env = c["env"].as_object().unwrap();
        let get = |k: &str| env.get(k).map(s);
        let input = CrashHeaderInput {
            now: ts_of(&c["ts"]),
            python_version: s(&c["python"]),
            pyside_version: opt_s(&c["pyside"]),
            os_platform: s(&c["os"]),
            xdg_current_desktop: get("XDG_CURRENT_DESKTOP"),
            desktop_session: get("DESKTOP_SESSION"),
            xdg_session_type: get("XDG_SESSION_TYPE"),
            lang: get("LANG"),
        };
        assert_eq!(qt_app::crash_header(&input), s(&c["out"]), "шапка [{name}]");
    }
}

/// `isoformat()` печатает дробную часть только при ненулевых микросекундах —
/// на этом расходятся два снятых случая.
#[test]
fn isoformat_drops_zero_microseconds() {
    let g = fx();
    let with = ts_of(&g["headers"]["full"]["ts"]);
    let without = ts_of(&g["headers"]["no_microseconds"]["ts"]);
    assert_eq!(with.isoformat(), "2026-07-30T14:05:09.123456");
    assert_eq!(without.isoformat(), "2026-01-02T03:04:05");
}

/// Рабочий стол берётся через `or` (пустая строка проваливается дальше), а
/// тип сессии и локаль — через `get(..., "?")`, где пустая строка остаётся
/// пустой. Асимметрия снята с живого Python.
#[test]
fn empty_env_values_differ_by_field() {
    let g = fx();
    let empty_de = s(&g["headers"]["empty_desktop"]["out"]);
    assert!(empty_de.contains("DE:       plasma (wayland)"), "{empty_de}");
    let empty_sess = s(&g["headers"]["empty_session_and_lang"]["out"]);
    assert!(empty_sess.contains("DE:       KDE ()"), "{empty_sess}");
    assert!(empty_sess.contains("Locale:   \n"), "{empty_sess}");
}

// ============================================================================
// Сохранение отчёта и ротация
// ============================================================================

/// Реализация [`CrashIo`] поверх карты «путь → содержимое». Реальная
/// файловая система в тестах не участвует.
struct MemFs {
    dirs: Vec<String>,
    files: HashMap<String, String>,
    makedirs_fails: bool,
    write_fails: bool,
    list_fails: bool,
    journal: Vec<String>,
}

impl MemFs {
    fn new(dir: &str, existing: &[String]) -> Self {
        let mut files = HashMap::new();
        for name in existing {
            files.insert(format!("{dir}/{name}"), "x".to_string());
        }
        MemFs {
            dirs: vec![dir.to_string()],
            files,
            makedirs_fails: false,
            write_fails: false,
            list_fails: false,
            journal: Vec::new(),
        }
    }

    fn names_in(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_string))
            .collect();
        out.sort();
        out
    }
}

impl CrashIo for MemFs {
    fn makedirs(&mut self, path: &str) -> bool {
        self.journal.push(format!("makedirs {path}"));
        if self.makedirs_fails {
            return false;
        }
        if !self.dirs.iter().any(|d| d == path) {
            self.dirs.push(path.to_string());
        }
        true
    }

    fn write_text(&mut self, path: &str, text: &str) -> bool {
        self.journal.push(format!("write {path}"));
        if self.write_fails {
            return false;
        }
        self.files.insert(path.to_string(), text.to_string());
        true
    }

    fn list_dir(&mut self, path: &str) -> Option<Vec<String>> {
        self.journal.push(format!("listdir {path}"));
        if self.list_fails {
            return None;
        }
        Some(self.names_in(path))
    }

    fn remove(&mut self, path: &str) {
        self.journal.push(format!("remove {path}"));
        self.files.remove(path);
    }
}

#[test]
fn save_crash_log_cases() {
    let g = fx();
    let cases = g["saves"].as_object().unwrap();
    assert_eq!(cases.len(), 4, "все снятые случаи сохранения должны сверяться");
    let dir = "/tmp/crashes";
    for (name, c) in cases {
        let existing: Vec<String> = c["existing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        let mut fs = MemFs::new(dir, &existing);
        let ts = ts_of(&c["ts"]);
        let path = qt_app::save_crash_log(&mut fs, dir, &ts, "HDR\n", s(&c["crash_text"]));
        assert_eq!(
            path.as_deref(),
            Some(format!("{dir}/{}", s(&c["name"])).as_str()),
            "путь отчёта [{name}]"
        );
        assert_eq!(
            fs.files[path.as_ref().unwrap()],
            s(&c["content"]),
            "содержимое отчёта [{name}]"
        );
        let rest: Vec<&str> = c["rest"].as_array().unwrap().iter().map(s).collect();
        assert_eq!(fs.names_in(dir), rest, "каталог после ротации [{name}]");
    }
}

#[test]
fn rotation_victims_cases() {
    let g = fx();
    for (name, c) in g["rotation"].as_object().unwrap() {
        let names: Vec<String> = c["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| s(v).to_string())
            .collect();
        let want: Vec<&str> = c["victims"].as_array().unwrap().iter().map(s).collect();
        assert_eq!(
            qt_app::rotation_victims(&names, qt_app::CRASH_KEEP),
            want,
            "ротация [{name}]"
        );
    }
}

/// Чужие файлы каталога ротация не трогает — в снятом случае уцелели
/// `README.txt`, `old.log` и `zz_crash_9.log` (последний содержит `crash_`,
/// но не НАЧИНАЕТСЯ с него).
#[test]
fn rotation_keeps_foreign_files() {
    let g = fx();
    let rest: Vec<&str> = g["saves"]["over_keep"]["rest"]
        .as_array()
        .unwrap()
        .iter()
        .map(s)
        .collect();
    for foreign in ["README.txt", "old.log", "zz_crash_9.log"] {
        assert!(rest.contains(&foreign), "пропал чужой файл {foreign}");
    }
}

#[test]
fn crash_log_name_and_dir() {
    let g = fx();
    let ts = ts_of(&g["saves"]["empty_dir"]["ts"]);
    assert_eq!(qt_app::crash_log_name(&ts), s(&g["saves"]["empty_dir"]["name"]));
    assert_eq!(qt_app::CRASH_KEEP as u64, g["crash_keep"].as_u64().unwrap());
    // CRASH_DIR оригинала снят на этой машине с пустым $XDG_CONFIG_HOME.
    assert_eq!(
        qt_app::crash_dir(None, "/home/user"),
        s(&g["crash_dir_expanded"])
    );
    assert_eq!(
        qt_app::crash_dir(Some("/run/cfg"), "/home/user"),
        "/run/cfg/rexon/crashes"
    );
}

/// Каталог не создался или запись не удалась — пути нет и ротация не
/// запускается.
#[test]
fn save_crash_log_failures() {
    let mut fs = MemFs::new("/tmp/crashes", &[]);
    fs.makedirs_fails = true;
    let ts = Timestamp::default();
    assert_eq!(
        qt_app::save_crash_log(&mut fs, "/tmp/crashes", &ts, "H", "T"),
        None
    );
    assert_eq!(fs.journal, vec!["makedirs /tmp/crashes"]);

    let mut fs = MemFs::new("/tmp/crashes", &[]);
    fs.write_fails = true;
    assert_eq!(
        qt_app::save_crash_log(&mut fs, "/tmp/crashes", &ts, "H", "T"),
        None
    );
    assert!(!fs.journal.iter().any(|j| j.starts_with("listdir")));
}

// ============================================================================
// Диалог о крахе
// ============================================================================

/// Разыграть журнал `_show_crash_dialog` из голдена по частям, собранным
/// ядром.
fn dialog_journal(header: &str, crash_text: &str, log_path: Option<&str>, clicked: usize) -> Vec<(String, Value)> {
    let d = qt_app::crash_dialog(header, crash_text, log_path);
    let mut out: Vec<(String, Value)> = vec![
        ("QMessageBox()".into(), Value::Null),
        ("setIcon".into(), Value::String("Icon.Critical".into())),
        ("setWindowTitle".into(), Value::String(d.title.clone())),
        ("setText".into(), Value::String(d.text.clone())),
    ];
    if let Some(inf) = &d.informative {
        out.push(("setInformativeText".into(), Value::String(inf.clone())));
    }
    out.push(("setDetailedText".into(), Value::String(d.detailed.clone())));
    for (_, label, role) in qt_app::crash_buttons() {
        out.push((
            "addButton".into(),
            Value::Array(vec![
                Value::String(label),
                Value::String(format!("ButtonRole.{}", role.as_str())),
            ]),
        ));
    }
    out.push(("exec".into(), Value::Null));
    let button = qt_app::CRASH_BUTTON_ORDER[clicked].0;
    match qt_app::crash_button_action(&d, button, log_path) {
        qt_app::CrashAction::Copy(text) => {
            out.push(("clipboard.setText".into(), Value::String(text)))
        }
        qt_app::CrashAction::Run(argv) => out.push((
            "Popen".into(),
            Value::Array(argv.into_iter().map(Value::String).collect()),
        )),
        qt_app::CrashAction::None => {}
    }
    out
}

#[test]
fn crash_dialog_cases() {
    // Эталон — русский текст окна Python, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases = g["dialog"].as_object().unwrap();
    assert_eq!(cases.len(), 4, "все снятые случаи диалога должны сверяться");
    for (name, c) in cases {
        let clicked = c["clicked"].as_u64().unwrap() as usize;
        let got = dialog_journal("HDR\n", "Traceback...\nValueError: боом\n", opt_s(&c["log_path"]), clicked);
        let want: Vec<(String, Value)> = c["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                let a = pair.as_array().unwrap();
                (s(&a[0]).to_string(), a[1].clone())
            })
            .collect();
        assert_eq!(got, want, "журнал диалога [{name}]");
    }
}

/// Текст в буфер — ровно то же, что в развёрнутой части окна.
#[test]
fn clipboard_text_equals_detailed() {
    let d = qt_app::crash_dialog("HDR\n", "T\n", Some("/tmp/c/crash_1.log"));
    match qt_app::crash_button_action(&d, CrashButton::Copy, Some("/tmp/c/crash_1.log")) {
        qt_app::CrashAction::Copy(t) => assert_eq!(t, d.detailed),
        other => panic!("ожидался Copy, получено {other:?}"),
    }
}

#[test]
fn open_folder_argv_dirname() {
    assert_eq!(
        qt_app::crash_open_folder_argv("/tmp/crashes/crash_1.log"),
        vec!["xdg-open", "/tmp/crashes"]
    );
    // Порт `os.path.dirname`: корень остаётся слэшем, имя без каталога даёт
    // пустую строку.
    assert_eq!(qt_app::crash_open_folder_argv("/crash.log")[1], "/");
    assert_eq!(qt_app::crash_open_folder_argv("crash.log")[1], "");
}

#[test]
fn button_roles_and_labels() {
    let _lang = i18n_pin::ru();
    let buttons = qt_app::crash_buttons();
    assert_eq!(buttons.len(), 3);
    assert_eq!(buttons[0].1, "Скопировать отчёт");
    assert_eq!(buttons[1].1, "Открыть папку");
    assert_eq!(buttons[2].1, "Закрыть");
    assert_eq!(ButtonRole::Action.as_str(), "ActionRole");
    assert_eq!(ButtonRole::Reject.as_str(), "RejectRole");
}

// ============================================================================
// Решение перехватчика исключений
// ============================================================================

/// Журнал живого Python: `stderr` → `sys.__excepthook__`, `save` →
/// `_save_crash_log`, `dialog` → `_show_crash_dialog`.
fn journal_kinds(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|pair| s(&pair.as_array().unwrap()[0]))
        .collect()
}

#[test]
fn crash_decision_matches_python() {
    let g = fx();
    let cases: &[(&str, &str, bool)] = &[
        ("main_value_error", "ValueError", true),
        ("main_keyboard_interrupt", "KeyboardInterrupt", true),
        ("thread_value_error", "ValueError", false),
        ("thread_keyboard_interrupt", "KeyboardInterrupt", false),
    ];
    for (case, exc, main_thread) in cases {
        let want = journal_kinds(&g["excepthook"][case]);
        let d = qt_app::crash_decision(exc, *main_thread);
        let mut got: Vec<&str> = Vec::new();
        if d.print_stderr {
            got.push("stderr");
        }
        if d.save_log {
            got.push("save");
        }
        if d.show_dialog {
            got.push("dialog");
        }
        assert_eq!(got, want, "решение [{case}]");
    }
}

/// Починка №2 (правило #16): `SystemExit` в оригинале проходил как крах —
/// живой Python на нём пишет отчёт и показывает диалог. Порт молчит.
/// `sys.exit()` в рабочем потоке доходит до `threading.excepthook`, то есть
/// штатный выход воркера показывал покупателю «Rexon неожиданно завершился».
#[test]
fn system_exit_is_silent_fix() {
    let g = fx();
    assert_eq!(
        journal_kinds(&g["excepthook"]["main_system_exit"]),
        vec!["stderr", "save", "dialog"],
        "эталон: Python считает SystemExit крахом"
    );
    let d = qt_app::crash_decision("SystemExit", false);
    assert!(!d.save_log && !d.show_dialog && !d.print_stderr);
    let d = qt_app::crash_decision("SystemExit", true);
    assert!(!d.save_log && !d.show_dialog && d.print_stderr);
}

/// Починка №3: крах в рабочем потоке требует переброса показа в поток GUI.
#[test]
fn worker_thread_dialog_hops_to_gui() {
    let main = qt_app::crash_decision("ValueError", true);
    assert!(main.show_dialog && !main.dialog_needs_main_thread_hop);
    let worker = qt_app::crash_decision("ValueError", false);
    assert!(worker.show_dialog && worker.dialog_needs_main_thread_hop);
}

// ============================================================================
// Платформа Qt и идентичность
// ============================================================================

#[test]
fn qt_qpa_platform_cases() {
    let g = fx();
    for (name, c) in g["qt_qpa_platform"].as_object().unwrap() {
        let input = opt_s(&c["in"]);
        let after = opt_s(&c["out"]);
        let unset = qt_app::should_unset_qt_platform(input);
        let got = if unset { None } else { input };
        assert_eq!(got, after, "QT_QPA_PLATFORM после импорта [{name}]");
    }
}

#[test]
fn identity_cases() {
    let g = fx();
    let cases = g["identities"].as_object().unwrap();
    assert_eq!(
        cases.len(),
        qt_app::ENTRY_IDENTITIES.len(),
        "таблица идентичностей должна покрывать все точки входа"
    );
    for (module, c) in cases {
        let id = qt_app::identity(module).unwrap_or_else(|| panic!("нет записи для {module}"));
        assert_eq!(
            id.application_name,
            s(&c["application_name"]),
            "applicationName [{module}]"
        );
        assert_eq!(
            id.display_name,
            opt_s(&c["display_name"]),
            "applicationDisplayName [{module}]"
        );
        assert_eq!(
            id.organization_name,
            opt_s(&c["organization_name"]),
            "organizationName [{module}]"
        );
        assert_eq!(
            id.desktop_file_name,
            opt_s(&c["desktop_file_name"]),
            "desktopFileName [{module}]"
        );
    }
    assert_eq!(qt_app::identity("rexon.qt_steam.app"), None);
}

/// У главного окна отображаемое имя — ПУСТАЯ строка, а не «не задано»:
/// KWin иначе приклеил бы «Rexon» в хвост каждого заголовка.
#[test]
fn main_window_display_name_is_empty_not_absent() {
    let id = qt_app::identity("rexon.qt_main.app").unwrap();
    assert_eq!(id.display_name, Some(""));
    assert_ne!(id.display_name, None);
}

/// Каждый флаг одиночной вкладки ведёт на модуль, у которого есть
/// идентичность (правило #12: маршрут и таблица не расходятся).
#[test]
fn every_single_tab_flag_has_identity() {
    for (flag, module) in rexon_core::app::SINGLE_TAB_FLAGS {
        assert!(
            qt_app::identity(module).is_some(),
            "флаг {flag} ведёт на {module} без идентичности"
        );
    }
}

// ============================================================================
// Диагностические строки
// ============================================================================

#[test]
fn banner_cases() {
    let g = fx();
    let b = &g["banners"];
    for case in ["platform_wayland", "platform_none", "platform_empty"] {
        let c = &b[case];
        assert_eq!(
            qt_app::qt_platform_banner(s(&c["platform"]), opt_s(&c["qpa"])),
            s(&c["out"]),
            "баннер платформы [{case}]"
        );
    }
    for case in ["window", "window_zero", "window_hex_letters"] {
        let c = &b[case];
        let r = c["rect"].as_array().unwrap();
        let rect = (
            r[0].as_i64().unwrap() as i32,
            r[1].as_i64().unwrap() as i32,
            r[2].as_i64().unwrap() as i32,
            r[3].as_i64().unwrap() as i32,
        );
        assert_eq!(
            qt_app::qt_window_banner(c["flags"].as_u64().unwrap(), rect),
            s(&c["out"]),
            "баннер окна [{case}]"
        );
    }
}

// ============================================================================
// Окно о крахе на языке интерфейса
// ============================================================================

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Всё, что окно о крахе показывает человеку (и Rust-окно, и системные
/// запасные), при данном адресе трекера.
fn crash_window_texts(url: Option<&str>) -> Vec<String> {
    let mut out = vec![
        qt_app::crash_dialog_title(),
        qt_app::crash_button_copy(),
        qt_app::crash_button_open(),
        qt_app::crash_button_close(),
        qt_app::crash_button_report(),
        qt_app::crash_details_show(),
        qt_app::crash_details_hide(),
        qt_app::crash_copied(),
    ];
    for fatal in [true, false] {
        out.push(qt_app::crash_dialog_text_for(fatal, url));
        for path in ["/c/crash_1.log", "/run/rexon-crash-1.log"] {
            let d = qt_app::crash_dialog_for_report("DETAILS", path, "/c", fatal);
            out.extend(d.informative.clone());
            for argv in qt_app::crash_fallback_argvs_for(&d, url) {
                out.extend(argv.into_iter().filter(|a| a != "DETAILS"));
            }
        }
    }
    out
}

/// Окно о крахе по-английски: ни одной русской буквы ни в окне, ни в
/// системных запасных окнах, с адресом трекера и без.
#[test]
fn english_crash_window_has_no_russian() {
    let _lang = i18n_pin::en();
    for url in [None, Some("https://example.org/issues")] {
        for text in crash_window_texts(url) {
            assert!(!has_cyrillic(&text), "{text}");
        }
    }
    assert_eq!(qt_app::crash_dialog_title(), "Rexon — An Error Occurred");
    assert_eq!(
        qt_app::crash_dialog_text(true),
        "Rexon quit unexpectedly.\n\nYou can copy the error report with the “Copy Report” \
         button and attach it to your message if you contact the developer."
    );
    let labels: Vec<String> = qt_app::crash_buttons().into_iter().map(|b| b.1).collect();
    assert_eq!(labels, ["Copy Report", "Open Folder", "Close"]);
    let d = qt_app::crash_dialog("H\n", "T", Some("/c/crash_1.log"));
    assert_eq!(d.informative.as_deref(), Some("Report saved:\n/c/crash_1.log"));
}

/// Окно о крахе по-русски — дословно как было до перевода (тексты тогда
/// были константами; здесь они выписаны буквами).
#[test]
fn russian_crash_window_reads_as_before() {
    let _lang = i18n_pin::ru();
    const URL: &str = "https://example.org/issues";
    let lead_fatal = "Rexon неожиданно завершился.";
    let lead_survived = "В Rexon произошла ошибка. Программа продолжает работать, но \
                         последнее действие могло не выполниться. Если что-то перестало \
                         откликаться — закройте Rexon и откройте снова.";
    let tail_plain = "\n\nОтчёт об ошибке можно скопировать кнопкой «Скопировать отчёт» \
                      и приложить к сообщению, если будете обращаться к разработчику.";
    let tail_tracker = "\n\nПомогите исправить ошибку: нажмите «Сообщить о проблеме», \
                        опишите на GitHub, что случилось, и вставьте в сообщение отчёт — \
                        его копирует кнопка «Скопировать отчёт».";
    assert_eq!(qt_app::crash_dialog_title(), "Rexon — произошла ошибка");
    assert_eq!(qt_app::crash_dialog_text_for(true, None), format!("{lead_fatal}{tail_plain}"));
    assert_eq!(qt_app::crash_dialog_text_for(false, None), format!("{lead_survived}{tail_plain}"));
    assert_eq!(qt_app::crash_dialog_text_for(true, Some(URL)), format!("{lead_fatal}{tail_tracker}"));
    assert_eq!(qt_app::crash_dialog_text_for(false, Some(URL)), format!("{lead_survived}{tail_tracker}"));
    assert_eq!(qt_app::crash_button_report(), "Сообщить о проблеме");
    assert_eq!(qt_app::crash_details_show(), "Показать подробности");
    assert_eq!(qt_app::crash_details_hide(), "Скрыть подробности");
    assert_eq!(
        qt_app::crash_copied(),
        "Отчёт скопирован — вставьте его в сообщение разработчику."
    );
    assert_eq!(
        qt_app::crash_fallback_file_hint(),
        "\n\nЕсли будете обращаться к разработчику, приложите к сообщению файл отчёта — путь ниже."
    );
    let saved = qt_app::crash_dialog_for_report("D", "/c/crash_1.log", "/c", true);
    assert_eq!(saved.informative.as_deref(), Some("Отчёт сохранён:\n/c/crash_1.log"));
    let with = qt_app::crash_fallback_argvs_for(&saved, Some(URL));
    assert_eq!(
        with[0][4],
        format!(
            "{lead_fatal}\n\nПомогите исправить ошибку: сообщите о ней на GitHub ({URL}) \
             и приложите к сообщению файл отчёта — путь ниже.\n\nОтчёт сохранён:\n/c/crash_1.log"
        )
    );
    let moved = qt_app::crash_dialog_for_report("D", "/run/rexon-crash-1.log", "/c", false);
    assert_eq!(
        qt_app::crash_fallback_argvs_for(&moved, Some(URL))[1][7],
        format!("{lead_survived}\n\nПомогите исправить ошибку: сообщите о ней на GitHub — {URL}")
    );
}
