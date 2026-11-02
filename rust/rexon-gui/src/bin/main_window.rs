// rexon-main-window — главное окно Rexon, у которого хозяин на Rust.
//
// ЗАЧЕМ ЭТОТ БИНАРЬ ВООБЩЕ ЕСТЬ. До него зависимость смотрела не туда:
// процесс поднимал Python (`rexon.py` → `rexon/app.py` →
// `rexon/qt_main/app.py`), он же создавал `QApplication` и `QMainWindow`, а
// Rust подключался к нему библиотекой через `ctypes.CDLL`. Бинаря главного
// окна на Rust не существовало — в `src/bin/` лежали только служебные
// (хост видео-обоев, хост веб-обоев, проба сокета, стенд моделей). Пока
// хозяин процесса — Python, уйти с него нельзя в принципе.
//
// Здесь хозяин — Rust: `QGuiApplication`, `QQmlApplicationEngine` и окно
// целиком на QML (`qml/MainWindow.qml`). Виджетов в процессе нет ни одного.
//
// ⭐ ЭТО ВХОД ПРОДУКТА. Ярлык рабочего стола (со всеми быстрыми действиями:
// `--page=`, `--recover-kde`) и автозапуск лотка ведут сюда — вход выбирает
// одна функция ядра (`installer::launch_entry_live`, коммит db4e33e). Окно на
// Python (`rexon.py` → `rexon/app.py` → `rexon/qt_main/**`) остаётся рядом
// отдельным пунктом меню ярлыка «Rexon (классическое окно)» и откатом:
// `REXON_ENTRY=python` либо отсутствие этого бинаря — и ярлык с
// автозапуском пишутся на Python-вход. Поэтому всё, что Python-хозяин
// делает при старте, обязано делать и это окно: иначе переход ярлыка
// молча отнял бы у покупателя часть продукта.
//
// Запуск:
//     cargo build-app && rust/target/release/rexon-main-window
// Открыть сразу нужный раздел (идентификаторы — `rexon_core::qt_main`):
//     rust/target/release/rexon-main-window --section=appearance
// Понимается и имя из ярлыка рабочего стола (`--page=install|backups|
// compare`), и пять флагов одиночных вкладок продукта (`--appearance-qt`,
// `--extensions-qt`, `--compare-qt`, `--settings-qt`, `--desktops-qt`) —
// тем же переводом, что у продукта (`rexon_core::cli::start_section`).
// Служебные маршруты без окна — `--apply-defaults`, `--sanitize-gtk`,
// `--session-splash`: их отрабатывает ядро и отдаёт КОД ВОЗВРАТА, который
// становится кодом возврата процесса.
// Прогон без окна (для скриптов):
//     QT_QPA_PLATFORM=offscreen rust/target/release/rexon-main-window --selftest
// Код возврата в режиме `--selftest`: 0 — все проверки прошли, 1 — хоть
// одна провалилась, 2 — окно не отчиталось за отведённое время.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString, QUrl,
    QVariant,
};
// ⭐ QApplication, а не QGuiApplication, и это единственная причина, по
// которой у окна на QML появился значок в лотке: QSystemTrayIcon, QMenu и
// QAction — виджеты, и Qt не создаёт их без QApplication вовсе. В самой
// cxx-qt-lib её нет ни в 0.9, ни в 0.10, зато есть в соседнем крейте того же
// выпуска — разбор в `rexon-gui/Cargo.toml`.
use cxx_qt_lib_extras::QApplication;

// Ссылка на библиотеку крейта обязательна, а не декоративна: без неё rustc
// не подтягивает rlib, а вместе с ним и статические архивы cxx-qt (модели,
// moc, QML-плагин с ресурсами) — бинарь собирается, но QML пуст.
use rexon_gui::main_window::qobject::MainWindowModel;

use rexon_core::{applog, cli, exit_guard, i18n, single_instance};
use rexon_core::{log_error, log_info, log_warn};

/// Признак прогона без глаз.
const SELFTEST_ARG: &str = "--selftest";

/// Сторож самопроверки. Если QML не загрузился или не отчитался, прогон
/// обязан ЗАВЕРШИТЬСЯ ОШИБКОЙ, а не тихим нулём: молчание скрипт примет за
/// успех.
const SELFTEST_TIMEOUT_SECS: u64 = 30;

// Две услуги Qt, которых в cxx-qt-lib 0.9 нет вовсе; шим — в
// `cpp/rexon_gui_factory.cpp`, там же разбор, почему они на C++. Сишная
// линковка вместо моста cxx: через границу ходят только строки и числа,
// мост был бы лишним звеном.
extern "C" {
    fn rexon_qt_translations_dir(buf: *mut c_char, len: c_int) -> c_int;
    fn rexon_install_qt_translations(paths: *const *const c_char, count: c_int) -> c_int;
    fn rexon_drain_thread_pool(ms: c_int) -> c_int;
    fn rexon_stop_media_players() -> c_int;
    fn rexon_qt_runtime_version() -> *const c_char;
    fn rexon_qt_build_version() -> *const c_char;
}

/// Сколько процесс может выходить после конца цикла событий, прежде чем
/// сторож завершит его принудительно. Сток фоновых задач ограничен 8 с
/// (`exit_guard::DEFAULT_BUDGET_MS`), остальное — секунды; зависший выход
/// держал замок «один Rexon на сессию», и перезапуск утыкался в него.
const EXIT_WATCHDOG_SECS: u64 = 20;

/// СТЕНД, не продукт: «зависнуть» на выходе на столько секунд (проверка
/// сторожа выхода и ожидания замка новой копией).
const STAND_EXIT_HANG_ENV: &str = "REXON_STAND_EXIT_HANG_SECS";

/// Задержка перед перезапуском в стенде цепочки, секунды.
const STAND_RESTART_DELAY_SECS: u64 = 8;

/// Шаг выхода, на котором процесс сейчас стоит, — для строки сторожа.
static EXIT_STEP: std::sync::Mutex<&'static str> = std::sync::Mutex::new("");

fn exit_step(step: &'static str) {
    if let Ok(mut s) = EXIT_STEP.lock() {
        *s = step;
    }
    rexon_core::log_debug!("exit", "шаг выхода: {step}");
}

/// Сторож выхода: не ушли за [`EXIT_WATCHDOG_SECS`] — строка в журнал (шаг
/// и живые потоки процесса) и `_exit` с тем же кодом. Без сторожа зависший
/// выход оставлял процесс-призрак без окна, держащий замок.
fn spawn_exit_watchdog(code: i32) {
    let _ = std::thread::Builder::new().name("rexon-exit-watch".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(EXIT_WATCHDOG_SECS));
        let step = EXIT_STEP.lock().map(|s| *s).unwrap_or("?");
        log_error!(
            "exit",
            "выход завис на шаге «{step}» дольше {EXIT_WATCHDOG_SECS} с — процесс завершается принудительно; живые потоки: {}",
            live_threads()
        );
        unsafe { libc::_exit(code) };
    });
}

/// Имена живых потоков процесса (`/proc/self/task/*/comm`) с повторами.
fn live_threads() -> String {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
                .map(|n| n.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut out: Vec<String> = Vec::new();
    for name in names {
        match out.last_mut() {
            Some(last) if last.starts_with(&format!("{name}×")) || *last == name => {
                let n = last.rsplit_once('×').and_then(|(_, n)| n.parse::<u32>().ok()).unwrap_or(1);
                *last = format!("{name}×{}", n + 1);
            }
            _ => out.push(name),
        }
    }
    out.join(", ")
}

fn c_text(p: *const c_char) -> String {
    if p.is_null() {
        return "?".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Строки запуска: кто, откуда, с какой Qt и на каком языке.
fn log_startup(args: &[String], language: &str, restart_of: Option<u32>) {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    log_info!(
        "start",
        "Rexon {} запущен: pid {}, родитель {}, {exe} {}",
        applog::version_line(),
        std::process::id(),
        unsafe { libc::getppid() },
        args.join(" ")
    );
    let (run, build) = unsafe { (c_text(rexon_qt_runtime_version()), c_text(rexon_qt_build_version())) };
    if run == build {
        log_info!("start", "Qt {run}");
    } else {
        log_warn!("start", "Qt рантайма {run}, а собран Rexon с Qt {build}");
    }
    log_info!("start", "окружение: {}", applog::desktop_env_line());
    if let Some(prev) = restart_of {
        log_info!("restart", "шаг 3: это новая копия, запущенная перезапуском процесса {prev}");
    }
    let saved = i18n::saved_setting();
    log_info!(
        "lang",
        "выбор «{saved}» → язык {language} (как в системе: {}); каталог {} (и для Rust, и для QML)",
        i18n::language_for_setting(None),
        i18n::mo_path().map_or_else(|| "нет".to_string(), |p| p.display().to_string())
    );
}

/// Сколько задач общего пула Qt не дождался сток выхода. Отдельно от общего
/// счёта — потому что в журнал идёт именно эта цифра: своих задач ноль, а
/// процесс всё равно не уходит, значит держит пул.
static POOL_LEFT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn main() {
    // Пробный запуск перед перезапуском (`settings_actions::probe_new_copy`):
    // сюда дошли — значит загрузчик привязал все символы, больше ничего не
    // нужно. Строго ПЕРВОЙ строкой: ни замка, ни журнала, ни Qt.
    if std::env::var_os(cli::LINK_PROBE_ENV).is_some() {
        std::process::exit(0);
    }

    // Перехватчик паники и окно отчёта о крахе — ПЕРВОЙ строкой: маршруты
    // без окна и замок одного экземпляра идут ниже, а `--crash-dialog`
    // показывает отчёт сам и на этом завершает процесс.
    rexon_gui::crash_report::install();

    let selftest = std::env::args().any(|a| a == SELFTEST_ARG);
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Журнал (`~/.local/state/rexon/rexon.log`) — до всего остального, чтобы
    // в него попал и язык, и замок. Самопроверка в профиль не пишет.
    if !selftest {
        applog::init();
        applog::install_fatal_signal_logger();
    }
    // Признак «я перезапуск» читается и СНИМАЕТСЯ: дети этого процесса (и
    // его собственный будущий перезапуск) не должны его унаследовать.
    let restart_of: Option<u32> = std::env::var(single_instance::RESTART_OF_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok());
    // Стенды (не продукт): цепочка перезапусков и «зависший выход». Обе
    // переменные снимаются, чтобы не уехать к детям сами по себе.
    let stand_restarts: u32 = std::env::var(rexon_gui::settings_actions::STAND_RESTARTS_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let stand_exit_hang: u64 = std::env::var(STAND_EXIT_HANG_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    unsafe {
        std::env::remove_var(single_instance::RESTART_OF_ENV);
        std::env::remove_var("REXON_RESTART_WATCH_TICKS");
        std::env::remove_var(rexon_gui::settings_actions::STAND_RESTARTS_ENV);
        std::env::remove_var(STAND_EXIT_HANG_ENV);
    }

    // Язык интерфейса — ДО первой строки для человека: служебные маршруты
    // ниже тоже говорят. Решение (настройка ▸ локаль ▸ английский) и поиск
    // каталога — за ядром (`rexon_core::i18n`).
    let language = rexon_core::i18n::init();
    if !selftest {
        log_startup(&args, &language, restart_of);
    }

    // ── Маршруты, которым окно не нужно ───────────────────────────────
    //
    // Одна дверь (`rexon_core::cli::run_headless`): бинарь не знает ни
    // одной ветки, ядро отвечает кодом возврата — и он ОБЯЗАН стать кодом
    // возврата процесса. Молчаливый ноль на неудавшемся autostart-хуке
    // скрипт принял бы за успех; оригинал отдавал 0/1 (`os._exit(rc)`).
    //
    // ⚠️ ДО замка «один Rexon на сессию»: служебные маршруты окна не
    // открывают, и упираться в чужой замок им незачем — иначе заставка при
    // входе не проигралась бы у того, у кого Rexon уже открыт.
    if !selftest {
        if let Some(rc) = cli::run_headless(&args, &mut |line| println!("{line}")) {
            std::process::exit(rc);
        }
    }

    // ── Окно, которое показать негде ──────────────────────────────────
    //
    // Дальше только окно. Без дисплея (SSH, текстовая консоль)
    // `QApplication` кончается `abort` — дамп памяти и ни слова человеку.
    // Сказать по-человечески и выйти с 1 — до замка: стук из консоли поднял
    // бы окно на чужом экране. Решение — за ядром (`cli::window_can_open`).
    if !cli::window_can_open(&|k| std::env::var(k).ok()) {
        log_error!("start", "окно показать негде: нет дисплея ({})", applog::desktop_env_line());
        for line in cli::no_display_lines() {
            eprintln!("{line}");
        }
        std::process::exit(1);
    }

    // Сборщики AUR (paru/yay) этого окна получают «sudo» — шим сеанса прав
    // (`rexon_core::privilege::aur`): их вызовы pacman идут в один пароль
    // окна. Служебным маршрутам выше и самопроверке он не нужен.
    if !selftest {
        rexon_core::privilege::aur::enable();
    }

    // Раздел, на котором открыть окно: `--section=` (вход разработчика),
    // флаг одиночной вкладки продукта либо имя из ярлыка рабочего стола.
    // Перевод целиком за ядром (`cli::start_section`), своей таблицы здесь
    // нет: с ней ярлык, флаги и бинарь разъехались бы молча.
    //
    // Подраздел («Сравнение DE» внутри «Рабочих столов», `--page=compare`
    // из ярлыка) ядро считает тем же вызовом, и он едет в QML начальным
    // свойством `startSubpage` — как у окна на Python (`show_compare`).
    let (start_section, start_subpage) = cli::start_section(&args).unwrap_or_default();

    // Битые ссылки на темы в gtk-{3,4}.0 роняют чужие GTK-приложения —
    // чиним до первого окна, как это делает старт окна на Python. Живой
    // санитайзер тот же, что у маршрута `--sanitize-gtk` (`cli`), второй
    // копии нет. Самопроверка профиль не трогает.
    if !selftest {
        cli::startup_gtk_sanitize(&mut |line| {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "gtk", line, false);
        });
    }

    // ── Один Rexon на сессию ──────────────────────────────────────────
    //
    // Замок и слушатель — общие с продуктом на Python
    // (`rexon_core::single_instance`, там же разбор обоих дефектов): адрес
    // один, поэтому запуск Rust-бинаря рядом с работающим Rexon НЕ поднимет
    // второе окно и второй значок в лотке, а постучится в первое.
    //
    // ⚠️ Самопроверка замок не берёт СПЕЦИАЛЬНО. `--selftest` проверяет
    // окно, а не второй экземпляр продукта: уткнувшись в чужой замок, он
    // молча вышел бы с нулём — и прогон, ни разу не построивший окна,
    // засчитался бы зелёным. Это ровно та подмена «тихий выход = успех»,
    // которую в проекте уже ловили у сторожа самопроверки ниже.
    if !selftest {
        let runtime_dir = single_instance::runtime_dir_from_env();
        let lock = single_instance::lock_path(&runtime_dir);
        let mut held = single_instance::acquire_lock(&lock);
        if held {
            log_info!("instance", "замок одного экземпляра взят: {lock}");
        } else {
            log_info!(
                "instance",
                "замок {lock} занят (держит процесс {})",
                single_instance::lock_holder_pid(&lock).map_or_else(|| "?".to_string(), |p| p.to_string())
            );
        }
        // Перезапуск: замок держит уходящая прежняя копия. Уйти сейчас —
        // ровно живой случай 08.10.2026 (новая постучалась в уходящую и
        // вышла, следом ушла и старая). Ждём её ухода сами.
        if let (false, Some(prev)) = (held, restart_of) {
            log_info!(
                "restart",
                "шаг 4: жду ухода прежнего процесса {prev} (до {} с)",
                single_instance::RESTART_LOCK_WAIT.as_secs()
            );
            match single_instance::wait_for_previous(&lock, prev, single_instance::RESTART_LOCK_WAIT) {
                single_instance::PreviousWait::Acquired(waited) => {
                    log_info!("restart", "шаг 5: прежний ушёл, замок взят через {} мс", waited.as_millis());
                    held = true;
                }
                single_instance::PreviousWait::PreviousStillRunning => log_error!(
                    "restart",
                    "прежний процесс {prev} не ушёл за {} с — стучусь в него, чтобы окно хотя бы было",
                    single_instance::RESTART_LOCK_WAIT.as_secs()
                ),
                single_instance::PreviousWait::OtherHolder(pid) => log_warn!(
                    "restart",
                    "прежний процесс {prev} ушёл, но замок держит другой ({}) — стучусь в него",
                    pid.map_or_else(|| "?".to_string(), |p| p.to_string())
                ),
            }
        }
        if !held {
            // Скрытый запуск (`--tray`) не стучится: стук вытащил бы уже
            // открытое окно на экран. Решение общее с Python-входом.
            let knocks =
                single_instance::on_busy_lock(&args) == single_instance::OnBusyLock::Knock;
            let sock = single_instance::socket_path(&runtime_dir);
            // Раздел быстрого действия ярлыка (`--page=backups` и т.п.) едет
            // в стуке: работающее окно откроет его, а не просто поднимется.
            // Что стучать, решает ядро (`knock_target`), как и у Python-входа.
            let target = single_instance::knock_target(&args);
            let target = target.as_ref().map(|(s, p)| (s.as_str(), p.as_str()));
            let outcome = knocks.then(|| single_instance::knock(&sock, target));
            // Настоящая причина молчания — в журнал; человеку — текст ядра.
            match &outcome {
                Some(single_instance::KnockOutcome::NoAnswer(reason)) => {
                    log_warn!("instance", "стук «подними окно» без ответа: {reason}")
                }
                Some(other) => log_info!("instance", "стук «подними окно»: {other:?}"),
                None => log_info!("instance", "скрытый запуск при занятом замке — выхожу молча"),
            }
            // Своего окна у этого запуска не будет, а Qt шлёт «запуск
            // окончен» только при показе окна: без этой строки панель MATE
            // держала «Запускается Rexon» и думающий курсор до таймаута
            // (`single_instance::complete_startup_notification`).
            match single_instance::complete_startup_notification() {
                Some((id, Ok(()))) => log_info!("instance", "уведомление о запуске завершено: {id}"),
                Some((id, Err(e))) => log_warn!("instance", "уведомление о запуске {id} не завершено: {e}"),
                None => {}
            }
            // Перезапуск, который так и не стал Rexon'ом, и окно никто не
            // поднял: человек нажал «Перезапустить» и остался ни с чем —
            // сказать уведомлением, раз окна у нас нет.
            if restart_of.is_some() && outcome != Some(single_instance::KnockOutcome::Raised) {
                log_error!("restart", "перезапуск не удался: окна нет ни у новой, ни у прежней копии");
                // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
                let summary = i18n::tr("Rexon didn't restart");
                let body = i18n::trf(
                    // TRANSLATORS: Body of a desktop notification; %1 is the path of the Rexon log file.
                    "The previous copy of Rexon didn't close in time. Open Rexon from the application menu; details are in the log: %1",
                    &[&applog::log_path().to_string_lossy()],
                );
                // Кнопка «Открыть Rexon» — то же, что ярлык меню, о котором
                // говорит текст (без кнопок текст остаётся единственным путём).
                let actions = vec![("open".to_string(), i18n::tr("Open Rexon"))];
                if rexon_core::utils::notify_ask_live(&summary, &body, &actions, "", 600).as_deref() == Some("open") {
                    if let Err(e) = rexon_core::installer::open_rexon_live(&[]) {
                        log_warn!("restart", "Rexon из уведомления не открылся: {e}");
                    }
                }
            }
            // Молчать здесь нельзя (правило #13): человек нажал ярлык и
            // обязан узнать, куда делась программа. Что сказать — решает ядро.
            for line in single_instance::said_on_busy_lock(&lock, outcome.as_ref()) {
                println!("{line}");
                applog::write_opts(applog::Level::Info, "lock", &line, false);
            }
            std::process::exit(0);
        }
    }

    // Без этого прогон из скрипта выглядит немым: когда stderr не терминал,
    // Qt на Arch/CachyOS уводит сообщения в journald, и строки самопроверки
    // из QML в вывод не попадают.
    //
    // У самопроверки — только эта переменная, БЕЗ формата и без перевода
    // дескриптора 2 в журнал: прогон читает строки из QML своим разбором, и
    // менять их вид под ним значило бы чинить одно, ломая другое.
    // Стандартные кнопки под KDE Plasma переводит тема платформы по
    // `LANGUAGE`, а не qtbase: без этой строки при английском Rexon на
    // русской системе диалог говорил «Да/Нет». Значение — решение ядра
    // (`cli::qt_ui_language_env`), ставится до создания приложения, пока
    // других потоков нет.
    // ⚠️ Через ядро, а не `set_var`: исходный `LANGUAGE` запоминается
    // (`i18n::original_locale_env`), и «как в системе» считается по нему, а
    // не по подменённому (живой случай 08.10.2026, «Wie im System (Deutsch)»).
    i18n::override_ui_language_env(&cli::qt_ui_language_env(&language));
    if !selftest {
        log_info!("lang", "переменные языка: {}", i18n::locale_env_report());
    }

    if selftest {
        unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    } else {
        // Сообщения Qt и QML — в журнал Rexon. Решение целиком за ядром
        // (`ui_base::qt_logging_env` + `should_redirect_stderr`); без этого
        // не попадало НИ ОДНО сообщение QML, и пустой журнал читался как
        // «ошибок нет».
        cli::install_qt_message_logger();
    }

    // ОБЯЗАТЕЛЬНО и не очевидно: статический QML-плагин с ресурсами и
    // регистрацией типов линкуется отдельным архивом, и без явной ссылки
    // линкер его выбрасывает — QML тогда падает на «qrc:/… No such file or
    // directory» ещё до всякой ошибки импорта.
    cxx_qt::init_qml_module!("org.rexon.gui");

    let mut app = QApplication::new();
    // Свои обои снимают живые обои Wallpaper Engine (решение ядра —
    // `wallpaper_exec::yields_we_wallpapers`). Сокет к демону живёт в Qt,
    // поэтому снимающего ставит окно, а не ядро.
    rexon_core::wallpaper_exec::set_we_stopper(rexon_gui::we_socket::stop_we_wallpapers_hook);
    // Представление системе — теми же строками, что у остальных точек входа
    // продукта (`rexon_core::qt_app`), а не второй копией здесь.
    //
    // Каждый сеттер отдельным блоком, как в соседнем стенде: `Pin<&mut …>`
    // не копируется, и два вызова подряд в одном блоке не собираются.
    if let Some(app) = app.as_mut() {
        app.set_organization_name(&QString::from(rexon_core::qt_app::ORGANIZATION_NAME));
    }
    if let Some(app) = app.as_mut() {
        // ⚠️ Имя приложения — МАШИННОЕ, а не заголовок окна: Qt отдаёт его на
        // шину свойством `Id` значка в лотке, а оболочка помнит по `Id`
        // настройки значка, сделанные покупателем. Разбор и сторож —
        // `rexon_core::tray::APP_ID`.
        app.set_application_name(&QString::from(rexon_core::tray::APP_ID));
    }
    // `setDesktopFileName` в Qt статический (он про весь процесс, а не про
    // объект приложения), и cxx-qt-lib оборачивает его так же.
    QGuiApplication::set_desktop_file_name(&QString::from(
        rexon_core::qt_app::DESKTOP_FILE_NAME,
    ));
    // Имени ярлыка хватает Wayland, а панель задач X11 (MATE, XFCE, Cinnamon)
    // берёт картинку из `_NET_WM_ICON` окна — значок ставится до первого окна
    // и действует на все окна процесса (`rexon_gui::tray::set_app_window_icon`).
    if !rexon_gui::tray::set_app_window_icon() && !selftest {
        log_warn!("start", "значок окна не найден ни в теме, ни файлом — панель задач покажет общий");
    }

    // Надписи на СОБСТВЕННЫХ кнопках Qt («OK», «Cancel», «Show Details…»,
    // меню правки в полях ввода) — на языке Rexon, а не системы: их рисует
    // сама Qt, и никакой наш перевод до них не достаёт.
    install_qt_translations(&language);
    install_app_translation(&language);

    // Вкладка обоев: каталог пакета, типы `waywallen.ui` (с синглтоном
    // `App`, он же ставит переводчик вендорных страниц), модель фильтров,
    // пускатель демона и прогрев Qt-Protobuf. Строго ПОСЛЕ приложения и ДО
    // первой загрузки QML — тот же порядок, что у питоновского хозяина, и
    // та же дверь (`rexon_gui::we_env`), второй копии здесь нет.
    rexon_gui::we_env::prepare_process();

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        // Пути импорта плагинов вкладки обоев, дисковый кэш миниатюр и
        // запись движка для смены языка вендорных страниц. Указатель —
        // на сам движок: `QQmlApplicationEngine` наследует `QQmlEngine`
        // первым (и единственным) предком, адрес у них общий.
        unsafe {
            let ptr = engine.as_mut().get_unchecked_mut() as *mut QQmlApplicationEngine;
            rexon_gui::we_env::prepare_engine(ptr.cast());
        }
        // Признак самопроверки едет в QML начальным свойством корня, а не
        // переменной контекста: контекстные свойства в cxx-qt-lib 0.9 не
        // обёрнуты, а `setInitialProperties` есть и работает ДО первой
        // привязки.
        let mut props = QMap::<QMapPair_QString_QVariant>::default();
        props.insert(QString::from("selftest"), QVariant::from(&selftest));
        // Окно грузится СПРЯТАННЫМ и показывается в конце запуска
        // (`tray::first_show_main_window`, там и замер 02.11.2026): с
        // `visible: true` из разметки оно выходило на экран посреди загрузки
        // и стояло прозрачным, пока поток GUI доделывал запуск. Самопроверка
        // окно не прячет — её прогон этого пути не касается.
        if !selftest {
            props.insert(QString::from("visible"), QVariant::from(&false));
        }
        props.insert(
            QString::from("startSection"),
            QVariant::from(&QString::from(&start_section)),
        );
        props.insert(
            QString::from("startSubpage"),
            QVariant::from(&QString::from(&start_subpage)),
        );
        props.insert(
            QString::from("weVendorQmlDir"),
            QVariant::from(&QString::from(&rexon_gui::we_env::vendor_qml_dir())),
        );
        engine.as_mut().set_initial_properties(&props);
        // Путь внутри ресурса повторяет путь файла в крейте: cxx-qt кладёт
        // .qml по относительному пути, а не в корень модуля.
        // Адрес движка — до `load`: та забирает закреплённую ссылку себе.
        let engine_ptr = unsafe { engine.as_mut().get_unchecked_mut() as *mut QQmlApplicationEngine };
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/MainWindow.qml"));
        if !selftest {
            // SAFETY: движок жив (владелец — `engine` до конца `main`) и
            // загружен; поток GUI.
            let adopted = unsafe { rexon_gui::tray::adopt_main_window(engine_ptr.cast()) };
            if !adopted {
                log_warn!("start", "корень QML не окно — показывать нечего");
            }
        }
    }

    // ── Значок в системном лотке ──────────────────────────────────────
    //
    // Разделы для подменю, файл картинки и флаг `--tray` дверь берёт из
    // ядра сама (`rexon_gui::tray::install_for_main_window`) — здесь ни
    // одного решения и ни одной подписи. Ставится ПОСЛЕ загрузки QML: значок
    // сразу берёт главное окно под сторожа закрытия, а до загрузки окна ещё
    // нет.
    //
    // Самопроверка значок НЕ ставит: она гоняет разделы и выходит, а лишний
    // значок в лотке живого сеанса — мусор на панели у человека.
    // Показать ли окно в конце запуска: прячется оно только при живом
    // значке и просьбе `--tray` (без значка окно — единственная дверь,
    // правило #13).
    let mut show_window = !selftest;
    if !selftest {
        let tray = rexon_gui::tray::install_for_main_window(&args);
        if tray && rexon_core::tray::starts_hidden(&args) {
            show_window = false;
        }
        // Просили свернуться, но значка нет — окно единственная дверь в
        // программу, и прятать его нельзя (правило #13). Дверь значка окно в
        // этом случае не прячет; сказать об этом всё равно надо — строку
        // добавляет ядро.
        for line in cli::tray_status_lines(tray, rexon_core::tray::starts_hidden(&args)) {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "tray", &line, false);
        }
        // Записи автозапуска со старым путём к программе (живой случай
        // 08.10.2026: `/usr/local/bin/rexon`) система молча пропускает —
        // чиним при каждом запуске, тихо и с записью в журнал. При живом
        // значке это делает его поток (перед освежением строк композиторов,
        // иначе два писателя одного файла), без значка — отдельный поток.
        if !tray {
            std::thread::spawn(rexon_gui::tray::repair_autostart_entries);
        }
        // Сторож оформления сеанса: тема, сменённая где угодно (в Rexon, в
        // окне «Внешний вид» MATE, в настройках любого окружения), получает
        // свои значки и курсор. Живёт, пока жив Rexon, со значком и без.
        rexon_core::theme_watch::spawn_once();
    }

    // Объявить Rexon в мост (`rexon_installed`, версия, окружение) — один
    // раз за запуск, как `announce_self` у окна на Python. Отдельным
    // потоком: запись файла не должна задерживать первую отрисовку. Мост не
    // критичен — неудача уходит строкой в журнал.
    if !selftest {
        std::thread::spawn(|| {
            if let Err(e) = cli::announce_to_bridge() {
                log_warn!("bridge", "объявление не записано: {e}");
            }
        });
    }

    // Обои Razer Axon — подготовить заранее, в фоне и с низким приоритетом,
    // чтобы «Применить» срабатывало сразу. С задержкой: запуску не мешать.
    // Выключено в настройках или Axon нет — поток выходит сразу.
    if !selftest {
        rexon_core::axon_prepare::spawn_background(rexon_core::axon_prepare::STARTUP_DELAY);
    }

    // Эталон защиты приложений по умолчанию — если он ещё ни разу не
    // снимался. Раздел включён по умолчанию, но до первого эталона Rexon
    // не помнит ничего и подмену не заметит, а нажимать «Запомнить сейчас»
    // покупатель не обязан. Решение и действие целиком в ядре
    // (`cli::startup_defaults_snapshot`), здесь одна строка; поток заводит
    // оно же — снятие опрашивает систему подпроцессами и в пути запуска
    // задержало бы первую отрисовку.
    //
    // ⚠️ Самопроверка эталон не снимает — именно эталон, а не «не трогает
    // профиль»: settings.json и каталог снимков она заводит и без нас, это
    // её давнее поведение. Обещать больше, чем делаешь, в комментарии
    // нельзя: следующий будет на это обещание опираться.
    if !selftest {
        cli::startup_defaults_snapshot(Box::new(|line: &str| {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "defaults", line, false);
        }));
    }

    // Слушатель «подними окно» встаёт ПОСЛЕ загрузки QML: до неё поднимать
    // ещё нечего, а сокет уже отвечал бы «ok» на пустоту.
    if !selftest {
        let runtime_dir = single_instance::runtime_dir_from_env();
        let sock = single_instance::socket_path(&runtime_dir);
        // Длинный `XDG_RUNTIME_DIR`: адрес ушёл в запасной каталог — это
        // знать надо и при удаче (где искать сокет), и тем более при неудаче.
        if let Some(note) = single_instance::socket_address_note(&runtime_dir) {
            log_info!("instance", "{note}");
        }
        // Держатель — Rust: на «уступи» от классического окна (`--classic`)
        // окно уходит, на «покажи раздел» — открывает его и поднимается.
        match single_instance::ActivationListener::spawn_for(
            &sock,
            single_instance::Holder::Rust,
            on_activation,
        ) {
            Ok(()) => log_info!("instance", "слушатель «подними окно» встал: {sock}"),
            // Не встал — не беда для этого запуска, но второй запуск теперь
            // скажет человеку, что Rexon не отвечает, и знать причину надо.
            Err(e) => log_warn!("instance", "слушатель «подними окно» не встал ({sock}): {e}"),
        }
    }

    if !selftest && stand_restarts > 0 {
        rexon_gui::settings_actions::STAND_RESTARTS_LEFT
            .store(stand_restarts, std::sync::atomic::Ordering::SeqCst);
        log_info!("restart", "стенд: перезапуск через {STAND_RESTART_DELAY_SECS} с (осталось {stand_restarts})");
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(STAND_RESTART_DELAY_SECS));
            rexon_gui::settings_actions::stand_restart_now();
        });
    }

    if selftest {
        eprintln!(
            "хозяин окна на Rust: {}",
            std::any::type_name::<MainWindowModel>()
        );
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(SELFTEST_TIMEOUT_SECS));
            eprintln!(
                "окно не отчиталось за {SELFTEST_TIMEOUT_SECS} с — самопроверка провалена"
            );
            std::process::exit(2);
        });
    }

    // Общий пул потоков Qt считает сток выхода, но своим счётчиком фоновых
    // задач его не видно: рабочие потоки пула заводит C++. Зацепка ставится
    // ДО цикла событий — после `exec()` регистрировать поздно.
    exit_guard::register_waiter(|ms| {
        let left = unsafe { rexon_drain_thread_pool(ms.min(c_int::MAX as u64) as c_int) };
        let left = left.max(0) as usize;
        POOL_LEFT.store(left, std::sync::atomic::Ordering::SeqCst);
        left
    });

    // Первый показ окна — последним делом перед циклом событий: дальше поток
    // GUI сразу рисует кадр, и прозрачного окна на экране не бывает.
    if !selftest {
        rexon_gui::tray::first_show_main_window(show_window);
    }

    // Код возврата цикла событий — это приговор самопроверки, и он обязан
    // стать кодом возврата процесса. Просто вернуть управление из main
    // нельзя: тогда провал выглядел бы успехом.
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    };
    let exit_started = std::time::Instant::now();
    if !selftest {
        log_info!(
            "exit",
            "цикл событий завершён (код {code}); причина: {}",
            rexon_gui::tray::quit_reason()
        );
    }
    spawn_exit_watchdog(code);
    exit_step("остановка проигрывателей");

    // ── Проигрыватели — стоп ДО всякого разбора ───────────────────────
    //
    // ⚠️ Роспуск движка (ниже) проигрыватели НЕ останавливает первыми:
    // `QQmlApplicationEngine` сносит окно, `~QQuickWindow` ждёт поток
    // отрисовки, а поток декодирования живого превью всё это время
    // продолжает работать. Поймано живьём 28.09.2026 (кордампы 1328272 и
    // 1332260, раздел «Steam» с играющей заставкой, два краха подряд):
    // посреди разбора у декодера VP9 на видеокарте NVIDIA (NVDEC) перестаёт
    // проходить `cuCtxPushCurrent`, и FFmpeg падает, печатая эту ошибку, —
    // SIGSEGV в `av_log` из `ff_cuda_check` (там читаются непроинициализи-
    // рованные указатели на имя ошибки). На стенде — 1 выход из 20 через
    // «Выход» из лотка. Потому сначала проигрыватели: остановить и снять
    // источник, что синхронно дожидается потоков FFmpeg, — и только потом
    // всё остальное. Порядок деструкторов Qt здесь ничего не обещает.
    let players = unsafe { rexon_stop_media_players() };
    if players > 0 {
        let line = cli::players_stopped_line(i64::from(players));
        println!("{line}");
        if !selftest {
            applog::write_opts(applog::Level::Info, "exit", &line, false);
        }
    }
    exit_step("сток фоновых задач");
    if stand_exit_hang > 0 {
        log_warn!("exit", "стенд: зависаю на выходе на {stand_exit_hang} с");
        std::thread::sleep(std::time::Duration::from_secs(stand_exit_hang));
    }

    // ── Сток фоновых задач ────────────────────────────────────────────
    //
    // Ровно здесь, а не в конце `main`: `exec()` уже вернул управление
    // (цикл событий решил закончиться), но ни `app`, ни `engine` ещё не
    // разобраны — это единственная точка, где выполнима честная
    // последовательность «попросить остановиться → дождаться → разобрать».
    // Всё, что позже, наступает уже после разрушения объектов Qt.
    //
    // Без этого `std::process::exit` обрывал фоновую задачу где придётся —
    // в том числе посреди записи файла темы, — а человек видел бы «Rexon
    // неожиданно завершился» там, где он просто закрыл окно.
    let left = exit_guard::stop_background_workers(exit_guard::DEFAULT_BUDGET_MS, true);
    if let Some(msg) = exit_guard::drain_report(
        left,
        POOL_LEFT.load(std::sync::atomic::Ordering::SeqCst),
    ) {
        // Молчать нельзя (правило #13): подвисший на выходе процесс человек
        // видит как «программа не закрывается», и причина обязана остаться
        // в журнале.
        log_warn!("exit", "{msg}");
    }

    // ⚠️ СНЯТЬ СЦЕНУ ДО ВЫХОДА, и именно в этом порядке. Выход из цикла
    // событий не разбирает построенное окно: движок QML продолжает держать
    // корневой объект, а с ним живые проигрыватели разделов — и их потоки
    // декодирования (`QFFmpeg::Stream`) продолжают работать. `exit` тогда
    // прокручивает обработчики завершения и выгружает библиотеки под
    // работающим потоком, и Rexon падает на самом выходе. Поймано живьём:
    // «Выход» из меню значка после открытия раздела «Steam» с
    // видеозаставкой — SIGSEGV в `av_log`; до того тем же классом падал
    // поток отрисовки сцены (`QSGRenderThread`). Роспуск движка сносит
    // корневой объект, тот — проигрыватели, те — свои потоки.
    //
    // Сток фоновых задач выше этого НЕ заменяет: он считает наши задачи, а
    // потоки сцены и мультимедиа заводит сама Qt.
    exit_step("роспуск движка QML");
    drop(engine);
    exit_step("роспуск приложения");
    drop(app);
    exit_step("завершение процесса");

    if !selftest {
        log_info!("exit", "выход за {} мс, код {code}", exit_started.elapsed().as_millis());
    }
    std::process::exit(code);
}

/// Включить каталоги переводов самой Qt для языка Rexon `language`.
///
/// Решение — за ядром (`cli::qt_translation_files`): какие файлы, в каком
/// порядке, и что у английского (и у языка, которого у Qt нет) их нет
/// вовсе. Здесь только два вызова шима — узнать каталог переводов Qt и
/// включить названные файлы: обёрток `QTranslator` и `QLibraryInfo` в
/// cxx-qt-lib 0.9 нет ни одной.
///
/// Молчание при неудаче намеренно: каталога `qt` нет в большинстве сборок
/// Qt6, и ругаться на его отсутствие значило бы пугать человека штатным
/// положением дел.
fn install_qt_translations(language: &str) {
    let dir = qt_translations_dir();
    let files = cli::qt_translation_files(language, std::path::Path::new(&dir), &|p| p.is_file());
    let owned: Vec<CString> = files
        .iter()
        .filter_map(|p| CString::new(p.to_string_lossy().as_bytes()).ok())
        .collect();
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    unsafe { rexon_install_qt_translations(ptrs.as_ptr(), ptrs.len() as c_int) };
}

/// Каталог переводов самой Qt (`QLibraryInfo::TranslationsPath`); пусто —
/// шим не ответил, и тогда ни один файл не найдётся (английские кнопки).
fn qt_translations_dir() -> String {
    let mut buf = vec![0u8; 512];
    loop {
        let need = unsafe { rexon_qt_translations_dir(buf.as_mut_ptr().cast(), buf.len() as c_int) };
        let need = usize::try_from(need).unwrap_or(0);
        if need < buf.len() {
            buf.truncate(need);
            return String::from_utf8_lossy(&buf).into_owned();
        }
        buf.resize(need + 1, 0);
    }
}

/// Перевод строк QML (`qsTranslate("rexon", …)`) — тот же язык и ТОТ ЖЕ
/// каталог `.mo`, что ядро уже выбрало для строк на Rust: переводчик окна
/// спрашивает ядро (`rexon_gui::install_app_translator`), своего
/// `.qm` у Rexon нет.
///
/// Английский (исходный) каталога не имеет, и это не ошибка: переводчик
/// тогда не ставится.
fn install_app_translation(language: &str) {
    if !rexon_gui::install_app_translator() {
        log_warn!("lang", "переводчик строк QML не встал (нет приложения Qt) — подписи QML останутся английскими");
    } else if language != rexon_core::i18n::SOURCE_LANGUAGE && rexon_core::i18n::mo_path().is_none() {
        log_warn!("lang", "для языка «{language}» нет rexon.mo — подписи останутся английскими");
    }
}

/// Поднять окно этого экземпляра по просьбе второго запуска.
///
/// Зовётся ИЗ ПОТОКА СЛУШАТЕЛЯ (`ActivationListener::spawn`), а окно у Qt
/// разрешено трогать только из потока GUI — значит поднимать его отсюда
/// нельзя ни при каких условиях, и «сделаю как получится» здесь означало бы
/// падение продукта на ровном месте.
///
/// ⭐ ДВЕРЬ ОТКРЫТА. Раньше здесь стояла заглушка `false`: у окна на QML не
/// было пути к самому окну — виджетов в процессе нет, а `cxx-qt-lib` не
/// отдаёт наружу ни корневой объект `QQmlApplicationEngine`, ни `QWindow`.
/// Путь нашёлся у шима лотка: он ищет окно среди `topLevelWindows()`
/// (QQuickWindow там есть) и сам переносит работу в поток GUI очередью с
/// ожиданием ответа. Поэтому звать отсюда МОЖНО, и ответ честный: `false`
/// значит «окна действительно нет», а не «не умею».
fn raise_window() -> bool {
    rexon_gui::tray::raise_main_window()
}

/// Обработчик слушателя активации (решение — `single_instance::listener_action`).
///
/// Зовётся из потока слушателя; двери лотка сами переносят работу в поток
/// GUI. На `HandOver` выход только ПЛАНИРУЕТСЯ и возвращается `true`:
/// ответ «ухожу» слушатель отправит после возврата, а выйти раньше ответа
/// значило бы оборвать классическому окну соединение.
fn on_activation(action: single_instance::ListenerAction) -> bool {
    match action {
        single_instance::ListenerAction::Raise(section) => {
            if let Some((id, subpage)) = &section {
                if !rexon_gui::tray::open_section(id, subpage) {
                    log_warn!("instance", "раздел «{id}» по просьбе второго запуска не открылся");
                }
            }
            let raised = raise_window();
            log_info!("instance", "второй запуск: поднять окно (раздел {section:?}) → {raised}");
            raised
        }
        single_instance::ListenerAction::HandOver => {
            rexon_gui::tray::request_quit("передача хозяина классическому окну");
            true
        }
    }
}
