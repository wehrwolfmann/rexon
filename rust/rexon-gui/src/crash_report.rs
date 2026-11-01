//! Отчёт о крахе Rust-окна (`rexon-main-window`).
//!
//! У Python-хозяина отчёт был (`rexon/qt_main/app.py`: `sys.excepthook` →
//! файл в `~/.config/rexon/crashes` + окно), у Rust-окна не было ничего: при
//! панике окно просто исчезало, и покупатель не узнавал ни что случилось, ни
//! что отправить разработчику. Здесь — то же для паник.
//!
//! ГРАНИЦА. Каталог, имя файла, шапка, ротация, тексты окна, решение «один
//! отчёт на процесс / смертельно ли» — ядро (`rexon_core::qt_app`), флаг
//! процесса окна — `rexon_core::cli`. Здесь только исполнение: перехватчик,
//! сбор сведений о системе, запуск окна и само окно.
//!
//! ПОЧЕМУ ОКНО — ОТДЕЛЬНЫЙ ПРОЦЕСС. Упавший процесс рисовать не может:
//!   * паника бывает в рабочем потоке, а Qt разрешает окна только в потоке
//!     GUI (у Python это лечилось перебросом сигналом — здесь его некому
//!     принять: цикл событий, возможно, уже мёртв);
//!   * паника в обработчике, который зовёт Qt, упирается в границу FFI, и
//!     процесс получает `abort` СРАЗУ после перехватчика — ни на какой
//!     цикл событий времени нет;
//!   * причиной паники может быть сама Qt в развале.
//!
//! Поэтому перехватчик пишет файл и запускает ТОТ ЖЕ бинарь с флагом
//! `--crash-dialog <файл>`: свежий процесс поднимает свою Qt и показывает
//! окно, а упавший спокойно умирает. Запускается `/proc/self/exe`, а не путь
//! на диске: при обновлении программы поверх работающей файл по пути уже
//! другой (или удалён), а образ процесса — тот самый.
//!
//! Процесс окна двухступенчатый: «сторож» (без Qt) запускает «окно» и ждёт
//! от него метку «показано». Метки нет — Qt не стартовала (`abort`), QML
//! не загрузился — сторож показывает системное окно (kdialog, затем
//! zenity). Нет дисплея вовсе — не показывается ничего: остаются файл и
//! строка в stderr.
//! Упавший процесс ждать не может — отсюда отдельный сторож. Не запустился
//! сам бинарь — системное окно пробует уже перехватчик.
//!
//! Подключение — одна строка первой в `main` бинаря:
//! `rexon_gui::crash_report::install();` — она же отрабатывает процесс окна
//! (и тогда из неё не возвращается).

use std::ffi::{c_char, CStr};
use std::os::unix::process::CommandExt;
use std::panic::PanicHookInfo;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use cxx_qt_lib::{
    QByteArray, QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString,
    QUrl, QVariant,
};
use rexon_core::{cli, qt_app, ui_base};

/// Не показывать окно о крахе (только файл и stderr). Для прогонов без
/// экрана: тесты, CI, скрипты. Любое непустое значение, кроме `0`.
pub const NO_DIALOG_ENV: &str = "REXON_CRASH_NO_DIALOG";

/// Ступень процесса окна: без переменной — сторож, `window` — само окно.
/// Переменная же отличает процесс окна от случайного ручного запуска с
/// флагом: такой запуск тоже идёт через сторожа, и это правильно.
const STAGE_ENV: &str = "REXON_CRASH_DIALOG_STAGE";
const STAGE_WINDOW: &str = "window";

/// Код возврата окна «QML не загрузился» — сторожу пора звать запасное.
const WINDOW_LOAD_FAILED: i32 = 3;

/// Строка, которой окно говорит сторожу «я на экране» (своим stdout,
/// сторож держит другой конец трубы). После неё запасные окна не зовутся,
/// чем бы окно ни кончилось: дисплей пропадает при выходе из сеанса, Qt
/// выходит с ошибкой — и без этой метки сторож звал бы kdialog туда, где
/// показать уже нечего (тот падает с `abort`, поймано живым прогоном).
const WINDOW_SHOWN_MARKER: &str = "crash-dialog-shown";

/// Отчёт в этом процессе уже был (см. `qt_app::panic_decision`).
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Поставить перехватчик паники. Звать ПЕРВОЙ строкой `main`.
///
/// Если этот процесс — окно о крахе (`--crash-dialog <файл>`), функция
/// показывает окно и завершает процесс, не возвращаясь: обычный запуск
/// (замок «один Rexon на сессию», лоток, главное окно) ему не нужен и
/// вреден — упавший экземпляр ещё может держать замок.
pub fn install() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(req) = cli::crash_dialog_request(&args) {
        // Строки окна о крахе («показать негде», «не поднялось») — в общий
        // журнал: этот процесс запускается без терминала, и его поток
        // ошибок иначе никто не прочтёт.
        rexon_core::applog::init();
        let code = if std::env::var(STAGE_ENV).as_deref() == Ok(STAGE_WINDOW) {
            run_window(&req)
        } else {
            supervise(&req, &args)
        };
        std::process::exit(code);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Штатная печать в stderr — всегда и первой: терминал и журнал
        // обязаны видеть панику, даже если отчёт дальше не запишется.
        previous(info);
        report_panic(info);
    }));
}

/// Тело перехватчика. Ни одного `unwrap`: паника внутри перехватчика — это
/// немедленный `abort` без отчёта.
fn report_panic(info: &PanicHookInfo) {
    let thread = std::thread::current();
    // TRANSLATORS: stands in the crash report for a thread that has no name.
    let unnamed = rexon_core::i18n::tr("<unnamed>");
    let thread_name = thread.name().unwrap_or(&unnamed);
    let decision = qt_app::panic_decision(
        thread.name() == Some("main"),
        REPORTED.swap(true, Ordering::SeqCst),
    );
    if !decision.save_log {
        return;
    }

    let message = info
        .payload_as_str()
        .map(str::to_string)
        // TRANSLATORS: stands in the crash report when the error has no message.
        .unwrap_or_else(|| rexon_core::i18n::tr("(panic without a message)"));
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    // Захват принудительный: переменная RUST_BACKTRACE у покупателя не
    // стоит, а отчёт без стека почти бесполезен.
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();

    let now = qt_app::Timestamp::now_local().unwrap_or_default();
    let header = live_header(now);
    let body = qt_app::panic_crash_text(&message, thread_name, location.as_deref(), &backtrace);
    let dir = crash_dir();

    let saved = qt_app::save_crash_log(&mut qt_app::AtomicCrashIo, &dir, &now, &header, &body);
    let path = saved.or_else(|| {
        // Каталог отчётов недоступен (нет места, чужие права) — окну отчёт
        // всё равно нужен, и он едет через временный файл.
        let p = qt_app::crash_transport_path(&runtime_dir(), std::process::id());
        qt_app::atomic_write(&p, &format!("{header}{body}")).ok().map(|_| p)
    });
    // Одна строка в общий журнал (`applog`): паника, где и куда лёг отчёт.
    // Штатная печать паники уже ушла в поток ошибок выше.
    rexon_core::log_error!(
        "crash",
        "паника в потоке «{thread_name}»{}: {message}; отчёт: {}",
        location.as_deref().map(|l| format!(" ({l})")).unwrap_or_default(),
        path.as_deref().unwrap_or("не записан")
    );

    if !decision.show_dialog || dialog_disabled() {
        return;
    }
    if !can_show() {
        rexon_core::log_warn!("crash", "окно о крахе показать негде: нет дисплея");
        return;
    }
    let launched = path
        .as_deref()
        .is_some_and(|p| spawn_dialog_process(p, decision.fatal));
    if !launched {
        // Свой бинарь не запустился (или показывать нечего по файлу) —
        // системное окно прямо отсюда, без ожидания.
        let report = format!("{header}{body}");
        let dialog = qt_app::crash_dialog_for_report(
            &report,
            path.as_deref().unwrap_or(""),
            &dir,
            decision.fatal,
        );
        for argv in qt_app::crash_fallback_argvs(&dialog) {
            if spawn_detached(Command::new(&argv[0]).args(&argv[1..]), decision.fatal) {
                break;
            }
        }
    }
}

fn can_show() -> bool {
    qt_app::crash_dialog_can_show(
        env_opt("WAYLAND_DISPLAY").as_deref(),
        env_opt("DISPLAY").as_deref(),
    )
}

fn dialog_disabled() -> bool {
    std::env::var_os(NO_DIALOG_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Запустить процесс окна о крахе (ступень «сторож»).
fn spawn_dialog_process(report_path: &str, fatal: bool) -> bool {
    let mut cmd = self_command();
    cmd.args(cli::crash_dialog_argv(report_path, fatal))
        .env_remove(STAGE_ENV)
        // Окно о крахе — на том же языке, что и упавшее окно. Настройки оно
        // само не читает (их поломка может быть причиной краха), поэтому
        // выбранный язык едет к нему окружением: `LANGUAGE` — первое, что
        // слушает выбор языка при пустой настройке (`i18n::language_candidates`).
        .env("LANGUAGE", rexon_core::i18n::current_language())
        // Метка «исходный LANGUAGE» перебила бы выбранный язык выше: окно о
        // крахе обязано взять именно его.
        .env_remove(rexon_core::i18n::ORIGINAL_LANGUAGE_ENV);
    spawn_detached(&mut cmd, fatal)
}

/// Команда на свой же образ (см. шапку модуля: `/proc/self/exe`, а не путь).
fn self_command() -> Command {
    let mut cmd = Command::new("/proc/self/exe");
    if let Some(arg0) = std::env::args_os().next() {
        cmd.arg0(arg0);
    }
    cmd
}

/// Запустить без ожидания, своей группой процессов: Ctrl+C в терминале,
/// которым гасят упавший процесс, не должен гасить окно о нём.
///
/// Если процесс выживет (`!fatal`), ребёнка дожидается отдельный поток —
/// иначе закрытое окно висело бы зомби до выхода Rexon.
fn spawn_detached(cmd: &mut Command, fatal: bool) -> bool {
    cmd.stdin(Stdio::null()).process_group(0);
    match cmd.spawn() {
        Ok(mut child) => {
            if !fatal {
                let _ = std::thread::Builder::new()
                    .name("crash-dialog-reaper".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
            true
        }
        Err(_) => false,
    }
}

// ============================================================================
// Сведения о системе для шапки
// ============================================================================

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn crash_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    qt_app::crash_dir(env_opt("XDG_CONFIG_HOME").as_deref(), &home)
}

fn runtime_dir() -> String {
    env_opt("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned())
}

extern "C" {
    /// Версия загруженной Qt. Сишная связь (объявлена в Qt внутри
    /// `extern "C"`), возвращает статическую строку — безопасно звать и
    /// тогда, когда сама Qt уже в развале.
    fn qVersion() -> *const c_char;
}

#[cfg(target_env = "gnu")]
extern "C" {
    fn gnu_get_libc_version() -> *const c_char;
}

fn c_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

fn libc_version() -> Option<(&'static str, String)> {
    #[cfg(target_env = "gnu")]
    {
        c_str(unsafe { gnu_get_libc_version() }).map(|v| ("glibc", v))
    }
    #[cfg(not(target_env = "gnu"))]
    {
        None
    }
}

fn read_trim(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Шапка отчёта по живой системе.
fn live_header(now: qt_app::Timestamp) -> String {
    let system = Some(read_trim("/proc/sys/kernel/ostype"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".to_string());
    let release = read_trim("/proc/sys/kernel/osrelease");
    let libc = libc_version();
    let os = qt_app::os_platform(
        &system,
        &release,
        std::env::consts::ARCH,
        libc.as_ref().map(|(n, v)| (*n, v.as_str())),
    );
    let entry = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let qt = c_str(unsafe { qVersion() });
    let (xcd, ds, st, lang) = (
        env_opt("XDG_CURRENT_DESKTOP"),
        env_opt("DESKTOP_SESSION"),
        env_opt("XDG_SESSION_TYPE"),
        env_opt("LANG"),
    );
    qt_app::rust_crash_header(&qt_app::RustCrashHeaderInput {
        now,
        rexon_version: env!("CARGO_PKG_VERSION"),
        entry: &entry,
        qt_version: qt.as_deref(),
        env: qt_app::CrashHeaderEnv {
            os_platform: &os,
            xdg_current_desktop: xcd.as_deref(),
            desktop_session: ds.as_deref(),
            xdg_session_type: st.as_deref(),
            lang: lang.as_deref(),
        },
    })
}

// ============================================================================
// Процесс окна о крахе
// ============================================================================

/// Содержимое окна по файлу отчёта. Файл не читается — окно всё равно
/// показывается: человек должен узнать, что программа упала.
fn load_dialog(req: &cli::CrashDialogRequest) -> qt_app::CrashDialog {
    let report = std::fs::read(&req.report_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_else(|e| {
            // TRANSLATORS: %1 is the report file path, %2 the system's error text.
            rexon_core::i18n::trf("(the report file can't be read: %1: %2)", &[&req.report_path, &e])
        });
    qt_app::crash_dialog_for_report(&report, &req.report_path, &crash_dir(), req.fatal)
}

/// Ступень «сторож»: запустить окно и дождаться; не показалось — запасные.
fn supervise(req: &cli::CrashDialogRequest, args: &[String]) -> i32 {
    if !can_show() {
        rexon_core::log_warn!("crash", "окно о крахе показать негде (нет дисплея); отчёт: {}", req.report_path);
        return 1;
    }
    match run_window_stage(args) {
        Ok(()) => return 0,
        Err(why) => rexon_core::log_warn!("crash", "окно о крахе не поднялось ({why}) — пробую системное"),
    }
    let dialog = load_dialog(req);
    for argv in qt_app::crash_fallback_argvs(&dialog) {
        match Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).status() {
            Ok(s) if s.success() => return 0,
            Ok(_) | Err(_) => continue,
        }
    }
    rexon_core::log_error!("crash", "окно о крахе показать нечем; отчёт: {}", req.report_path);
    1
}

/// Запустить ступень «окно» и дождаться. `Ok` — окно успело показаться
/// (метка [`WINDOW_SHOWN_MARKER`]), как бы оно потом ни завершилось.
fn run_window_stage(args: &[String]) -> Result<(), String> {
    use std::io::BufRead;
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    // Команда — временная и умирает в конце выражения вместе с копией
    // пишущего конца: иначе чтение ниже не дождалось бы конца трубы.
    let mut child = self_command()
        .args(args)
        .env(STAGE_ENV, STAGE_WINDOW)
        .stdin(Stdio::null())
        .stdout(writer)
        .spawn()
        .map_err(|e| e.to_string())?;
    // Только первая строка: после показа окно может запустить файловый
    // менеджер («Открыть папку»), и тот унаследует трубу — ждать её конца
    // значило бы висеть, пока человек не закроет файловый менеджер.
    let mut line = String::new();
    let _ = std::io::BufReader::new(reader).read_line(&mut line);
    let status = child.wait();
    if line.trim_end() == WINDOW_SHOWN_MARKER {
        return Ok(());
    }
    Err(match status {
        Ok(s) => s.to_string(),
        Err(e) => e.to_string(),
    })
}

/// Ступень «окно»: своя Qt, окно на QML, код 0 — человек его закрыл.
fn run_window(req: &cli::CrashDialogRequest) -> i32 {
    let dialog = load_dialog(req);
    // Пустая QT_QPA_PLATFORM мешает Qt выбрать wayland-плагин — то же
    // правило ядра, что у всех точек входа. Потоков ещё нет.
    if qt_app::should_unset_qt_platform(env_opt("QT_QPA_PLATFORM").as_deref()) {
        unsafe { std::env::remove_var("QT_QPA_PLATFORM") };
    }

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_organization_name(&QString::from(qt_app::ORGANIZATION_NAME));
    }
    if let Some(app) = app.as_mut() {
        // ⭐ ИМЯ ДЛЯ СИСТЕМЫ — МАШИННОЕ И ОДНО НА ПРОДУКТ. Окно о крахе —
        // отдельный процесс, и до сих пор он назывался Qt переведённым
        // заголовком ГЛАВНОГО окна (`qt_main::WINDOW_TITLE`): один продукт
        // приходил в систему двумя именами сразу — `Rexon` от главного окна и
        // строкой для человека отсюда. Дверь у имени одна — `rexon_core::tray::APP_ID`,
        // там же разбор, почему оно обязано быть постоянным и непереводимым.
        //
        // Значка в лотке у этого процесса нет, но имя приложения уходит и мимо
        // лотка: им подписаны записи краха в системном журнале, его берёт портал
        // и окна системы. Видимый заголовок самого окна от этого НЕ меняется:
        // человек читает `qt_app::crash_dialog_title()`, который приходит в QML
        // свойством `dialogTitle` и с именем приложения никак не связан.
        app.set_application_name(&QString::from(rexon_core::tray::APP_ID));
    }
    // Значок окна в панели задач X11 — тот же, что у главного окна.
    crate::tray::set_app_window_icon();
    QGuiApplication::set_desktop_file_name(&QString::from(qt_app::DESKTOP_FILE_NAME));

    let mut engine = QQmlApplicationEngine::new();
    let Some(mut engine) = engine.as_mut() else {
        return WINDOW_LOAD_FAILED;
    };

    // Окно в стандартном скине продукта. Скин по умолчанию, а не
    // выбранный в настройках: окно о крахе не имеет права зависеть от
    // чтения настроек — их поломка может оказаться причиной краха.
    let colors = ui_base::skin_colors(ui_base::DEFAULT_SKIN);
    let color = |k: &str| colors.get(k).cloned().unwrap_or_default();
    let folder = dialog
        .informative
        .as_ref()
        .map(|_| {
            let dir = qt_app::crash_open_folder_argv(&req.report_path)
                .pop()
                .unwrap_or_default();
            QUrl::from_local_file(&QString::from(&dir)).to_qstring()
        })
        .unwrap_or_default();

    let mut props = QMap::<QMapPair_QString_QVariant>::default();
    let mut put = |k: &str, v: &str| {
        props.insert(QString::from(k), QVariant::from(&QString::from(v)));
    };
    put("dialogTitle", &dialog.title);
    put("mainText", &dialog.text);
    put("informative", dialog.informative.as_deref().unwrap_or(""));
    put("detailed", &dialog.detailed);
    put("folderUrl", &folder.to_string());
    // Кнопка «Сообщить о проблеме» есть только тогда, когда у продукта
    // задан адрес трекера (`qt_app::ISSUE_TRACKER_URL`): пустой адрес —
    // пустое свойство, и QML прячет кнопку.
    let report = qt_app::crash_report_issue_button();
    let report_url = report
        .as_ref()
        .and_then(|(_, argv)| argv.last())
        .map(String::as_str)
        .unwrap_or("");
    put("reportLabel", report.as_ref().map(|(l, _)| l.as_str()).unwrap_or(""));
    put("reportUrl", report_url);
    put("copyLabel", &qt_app::crash_button_copy());
    put("openLabel", &qt_app::crash_button_open());
    put("closeLabel", &qt_app::crash_button_close());
    put("showDetailsLabel", &qt_app::crash_details_show());
    put("hideDetailsLabel", &qt_app::crash_details_hide());
    put("copiedLabel", &qt_app::crash_copied());
    put("bg", &color("bg_primary"));
    put("bgField", &color("bg_secondary"));
    put("bgCard", &color("bg_card"));
    put("bgCardHover", &color("bg_card_hover"));
    put("accent", &color("accent"));
    put("accentGreen", &color("accent_green"));
    put("accentGreenHover", &color("accent_green_hover"));
    put("textColor", &color("text_primary"));
    put("borderColor", &color("border"));
    engine.as_mut().set_initial_properties(&props);

    let loaded = std::sync::Arc::new(AtomicBool::new(false));
    let seen = loaded.clone();
    let _guard = engine.as_mut().on_object_created(move |_engine, object, _url| {
        if !object.is_null() {
            seen.store(true, Ordering::SeqCst);
        }
    });
    engine
        .as_mut()
        .load_data(&QByteArray::from(DIALOG_QML), &QUrl::from(DIALOG_QML_URL));
    if !loaded.load(Ordering::SeqCst) {
        rexon_core::log_error!("crash", "окно о крахе: QML не загрузился");
        return WINDOW_LOAD_FAILED;
    }
    println!("{WINDOW_SHOWN_MARKER}");
    match app.as_mut() {
        Some(app) => app.exec(),
        None => WINDOW_LOAD_FAILED,
    }
}

/// Адрес, под которым движок знает разметку окна (для сообщений об
/// ошибках; относительных импортов у окна нет).
const DIALOG_QML_URL: &str = "qrc:/rexon/crash/CrashDialog.qml";

/// Разметка окна о крахе. Строкой, а не файлом модуля `org.rexon.gui`:
/// окну не нужен ни один наш тип, а статический QML-модуль продукта в
/// процессе окна — лишняя точка отказа. Ни одной человеческой строки здесь
/// не набрано — все приходят из ядра свойствами.
///
/// Стиль Controls — Basic явно: у системного стиля KDE своё оформление
/// кнопок, и настройка их фона там не действует.
const DIALOG_QML: &str = r##"
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ApplicationWindow {
    id: root

    required property string dialogTitle
    required property string mainText
    required property string informative
    required property string detailed
    required property string folderUrl
    required property string reportLabel
    required property string reportUrl
    required property string copyLabel
    required property string openLabel
    required property string closeLabel
    required property string showDetailsLabel
    required property string hideDetailsLabel
    required property string copiedLabel
    required property color bg
    required property color bgField
    required property color bgCard
    required property color bgCardHover
    required property color accent
    required property color accentGreen
    required property color accentGreenHover
    required property color textColor
    required property color borderColor

    property bool detailsOpen: false
    property bool copied: false

    visible: true
    title: dialogTitle
    color: bg
    width: 640
    minimumWidth: 480
    height: content.implicitHeight + 48
    minimumHeight: content.implicitHeight + 48


    component DialogButton: Button {
        id: control
        property bool primary: false
        implicitHeight: 42
        leftPadding: 18
        rightPadding: 18
        font.pixelSize: 15
        contentItem: Text {
            text: control.text
            font: control.font
            color: root.textColor
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 3
            color: control.primary
                   ? (control.hovered ? root.accentGreenHover : root.accentGreen)
                   : (control.hovered ? root.bgCardHover : root.bgCard)
            border.width: control.activeFocus ? 2 : 1
            border.color: control.activeFocus ? root.accent : root.borderColor
        }
    }

    ColumnLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 24
        spacing: 14
        // Esc закрывает окно. Обработчик клавиш, а не Shortcut: Shortcut
        // срабатывает только в АКТИВНОМ окне, а окно о крахе появляется из
        // фонового процесса и активным бывает не всегда; клавиша же доходит
        // сюда по цепочке фокуса от кнопки.
        Keys.onEscapePressed: Qt.quit()

        RowLayout {
            spacing: 14
            Layout.fillWidth: true
            Rectangle {
                width: 40
                height: 40
                radius: 20
                color: "#c0392b"
                Text {
                    anchors.centerIn: parent
                    text: "!"
                    color: "#ffffff"
                    font.pixelSize: 24
                    font.bold: true
                }
            }
            Text {
                text: root.dialogTitle
                color: root.textColor
                font.pixelSize: 20
                font.bold: true
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        Text {
            text: root.mainText
            color: root.textColor
            font.pixelSize: 16
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        TextEdit {
            visible: root.informative !== ""
            text: root.informative
            color: root.textColor
            opacity: 0.85
            font.pixelSize: 15
            readOnly: true
            selectByMouse: true
            wrapMode: TextEdit.Wrap
            Layout.fillWidth: true
        }

        DialogButton {
            text: root.detailsOpen ? root.hideDetailsLabel : root.showDetailsLabel
            onClicked: root.detailsOpen = !root.detailsOpen
        }

        ScrollView {
            visible: root.detailsOpen
            Layout.fillWidth: true
            Layout.preferredHeight: 280
            TextArea {
                id: details
                text: root.detailed
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.NoWrap
                color: root.textColor
                font.family: "monospace"
                font.pixelSize: 12
                background: Rectangle {
                    color: root.bgField
                    border.color: root.borderColor
                }
            }
        }

        Text {
            visible: root.copied
            text: root.copiedLabel
            color: root.accent
            font.pixelSize: 15
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Item { Layout.fillWidth: true }
            DialogButton {
                visible: root.reportUrl !== ""
                text: root.reportLabel
                onClicked: Qt.openUrlExternally(root.reportUrl)
            }
            DialogButton {
                id: copyButton
                primary: true
                text: root.copyLabel
                onClicked: {
                    details.selectAll();
                    details.copy();
                    details.deselect();
                    root.copied = true;
                }
            }
            DialogButton {
                visible: root.folderUrl !== ""
                text: root.openLabel
                onClicked: Qt.openUrlExternally(root.folderUrl)
            }
            DialogButton {
                text: root.closeLabel
                onClicked: Qt.quit()
            }
        }
    }

    Component.onCompleted: copyButton.forceActiveFocus()
}
"##;

// ============================================================================
// Сторожа идентичности
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐Окно о крахе — отдельный процесс продукта, и системе оно обязано
    /// называться ТЕМ ЖЕ машинным именем, что главное окно и стенд лотка
    /// (`rexon_core::tray::APP_ID`; разбор — у самой константы). Живой
    /// дефект: сюда попадал переведённый заголовок главного окна, и один
    /// продукт приходил в систему двумя именами сразу.
    ///
    /// Сторож смотрит в ИСХОДНИК, а не в значение: значение само с собой
    /// всегда сойдётся, а разъехаться может ровно вызов.
    #[test]
    fn имя_для_системы_берётся_из_общей_двери_а_не_из_заголовка() {
        let src = include_str!("crash_report.rs");
        let call = src
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("app.set_application_name("))
            .expect("ступень «окно» называет себя Qt");
        assert!(
            call.contains("rexon_core::tray::APP_ID"),
            "имя для системы взято не из общей двери: {call}"
        );
        assert!(
            !call.contains("WINDOW_TITLE"),
            "в имя для системы снова уехал заголовок окна: {call}"
        );
        // Само имя — машинное: одно слово латиницей, а не подпись для человека.
        assert!(rexon_core::tray::APP_ID.is_ascii());
        assert!(!rexon_core::tray::APP_ID.contains(' '));
    }

    /// А человек по-прежнему читает человеческое: видимый заголовок окна —
    /// строка ядра для покупателя, и от машинного имени он не зависит.
    #[test]
    fn видимый_заголовок_окна_падения_остался_человеческим() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let dir = "/home/u/.config/rexon/crashes";
        let dialog =
            qt_app::crash_dialog_for_report("ОТЧЁТ", &format!("{dir}/crash_1.log"), dir, true);
        assert_eq!(dialog.title, qt_app::crash_dialog_title());
        assert_eq!(dialog.title, "Rexon — An Error Occurred");
        assert_ne!(dialog.title, rexon_core::tray::APP_ID);
        assert!(
            dialog.title.contains(' '),
            "заголовок для человека — фраза, а не машинное имя: {}",
            dialog.title
        );
        // Заголовок окна берётся из этого свойства, а не из имени приложения.
        assert!(DIALOG_QML.contains("title: dialogTitle"));
        assert!(!DIALOG_QML.contains("Qt.application"));
    }
}
