//! Единая дверь точки входа Rexon — порт тела `main()` из `rexon/app.py`
//! (ветки маршрутов) плюс то, что Python делал ПЕРЕД поднятием Qt.
//!
//! РАЗДЕЛЕНИЕ ТРУДА. [`crate::app::route`] отвечает «что делать», этот
//! модуль — «сделать это на живой машине и с каким кодом выйти». Бинарь
//! (`rexon-gui/src/bin/main_window.rs`) не знает ни одной ветки: он зовёт
//! [`run_headless`], и если та ответила кодом — выходит с ним, а если
//! `None` — поднимает Qt на разделе из [`start_section`]. Правило одной
//! двери: новая ветка появляется ЗДЕСЬ, бинарь не правится.
//!
//! ЧТО ЗАКРЫВАЕТ. До этого модуля Rust-бинарь понимал только `--section=` и
//! `--page=`, а служебные флаги продукта — `--apply-defaults`,
//! `--sanitize-gtk`, `--session-splash`, пять флагов одиночных вкладок —
//! молча уходили в никуда: Qt незнакомый аргумент игнорирует, и бинарь
//! открывал главное окно вместо обещанной работы. Хуже того, КОДОВ ВОЗВРАТА
//! маршрутов он не отдавал вовсе: autostart-хук `--apply-defaults`
//! отчитывался нулём при неудаче применения дефолтов, то есть скрипт видел
//! успех там, где ничего не применилось. Оригинал возвращал 0/1 (`os._exit(rc)`
//! в `rexon.py`), и это ровно та подмена «тихий выход = успех», которую в
//! проекте уже ловили у сторожа самопроверки окна.
//!
//! ⭐ `--recover-kde` ЗАКРЫТ ЗДЕСЬ ЖЕ. Последний маршрут, который упирался в
//! интерпретатор, теперь целиком на Rust: разбор аргумента — [`crate::app`],
//! порядок шагов и походы наружу —
//! [`crate::system_exec::SystemExec::recover_kde`], код возврата — отсюда.
//! Это и был последний крупный пробел точки входа: пока аварийный откат
//! оформления умел только Python, Rust-бинарь не мог стать `/usr/bin/rexon`
//! — по ярлыку «Восстановить оформление рабочего стола» человек получил бы
//! отказ ровно в тот момент, когда оформление уже сломано.

use std::path::{Path, PathBuf};

use crate::app::Route;
use crate::i18n::{ntr, tr, trf};

// ============================================================================
// Одиночные Qt-вкладки → раздел главного окна
// ============================================================================
//
// В оригинале каждый из пяти флагов поднимал ОТДЕЛЬНОЕ приложение с одним
// окном — это было единственным способом открыть одну вкладку, пока вкладки
// были разными процессами. У Rust-бинаря все шесть разделов живут в одном
// окне, и «одна вкладка» — это то же окно, открытое на нужном разделе, то
// есть уже существующий `--section=`. Поэтому пять модулей сводятся к
// таблице «флаг → раздел», а не к пяти точкам входа: вторая копия состава
// разделов разъехалась бы с первой молча (правило #12).

/// Модуль одиночной вкладки → (раздел окна, подраздел внутри него).
///
/// Ключи — значения [`crate::app::SINGLE_TAB_FLAGS`], значения — разделы из
/// [`crate::qt_main::SECTIONS`]; сверка обоих списков в
/// `tests/cli_parity.rs`.
///
/// «Сравнение DE» отдельным разделом не идёт — с 08.07.2026 оно живёт ВНУТРИ
/// «Рабочих столов», поэтому у `qt_compare` тот же раздел, что у
/// `qt_desktops`, и отличает их подраздел (ровно как у `--page=compare`).
pub const SINGLE_TAB_SECTIONS: &[(&str, &str, &str)] = &[
    ("rexon.appearance_page", "appearance", ""),
    ("rexon.qt_extensions.app", "extensions", ""),
    ("rexon.qt_compare.app", "desktops", "compare"),
    ("rexon.qt_settings.app", "settings", ""),
    ("rexon.qt_desktops.app", "desktops", ""),
];

/// Раздел и подраздел для модуля одиночной вкладки.
pub fn single_tab_section(module: &str) -> Option<(&'static str, &'static str)> {
    SINGLE_TAB_SECTIONS
        .iter()
        .find(|(m, _, _)| *m == module)
        .map(|(_, section, subpage)| (*section, *subpage))
}

/// Раздел, на котором обязано открыться окно по всей командной строке.
///
/// Три источника в порядке убывания точности: `--section=<id>` (вход
/// разработчика, идентификатор как есть), флаг одиночной вкладки продукта и
/// `--page=<имя>` из ярлыка рабочего стола. Ни один не сработал — `None`,
/// и окно откроется на разделе по умолчанию.
///
/// `--section=` первым не по старшинству, а потому что это единственный
/// источник, у которого идентификатор задан буквально: остальные два —
/// переводы, и переводить то, что уже назвали точно, значит терять точность.
pub fn start_section(args: &[String]) -> Option<(String, String)> {
    // Окно из уведомления о поломке AeroThemePlasma — на «Настройках»: там
    // проверка оформления и её итог (сама проверка начинается сразу).
    if args.iter().any(|a| a == crate::aeroshell::OPEN_FLAG) {
        return Some(("settings".to_string(), String::new()));
    }
    if let Some(id) = args.iter().find_map(|a| a.strip_prefix(SECTION_FLAG)) {
        let id = id.trim();
        if !id.is_empty() {
            return Some((id.to_string(), String::new()));
        }
    }
    if let Route::SingleTab { module } = crate::app::route(args) {
        if let Some((section, subpage)) = single_tab_section(module) {
            return Some((section.to_string(), subpage.to_string()));
        }
    }
    crate::app::start_page(args).map(|(s, p)| (s.to_string(), p.to_string()))
}

/// Флаг раздела: `--section=appearance`.
pub const SECTION_FLAG: &str = "--section=";

// ============================================================================
// Окно о крахе отдельным процессом
// ============================================================================
//
// Упавший процесс окно о крахе не рисует: паника бывает в чужом потоке и в
// состоянии, где Qt уже неработоспособна, а паника через границу с Qt — это
// `abort` сразу после перехватчика. Поэтому перехватчик
// (`rexon-gui::crash_report`) запускает ТОТ ЖЕ бинарь с этими флагами, и
// свежий процесс показывает окно по файлу отчёта.

/// Флаг процесса окна о крахе: `--crash-dialog <файл отчёта>`.
pub const CRASH_DIALOG_FLAG: &str = "--crash-dialog";

/// Добавка к [`CRASH_DIALOG_FLAG`]: упавший процесс выжил (паника в рабочем
/// потоке), окно говорит «продолжает работать», а не «завершился».
pub const CRASH_NONFATAL_FLAG: &str = "--crash-nonfatal";

/// Запрос на окно о крахе из командной строки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDialogRequest {
    pub report_path: String,
    pub fatal: bool,
}

/// Разобрать `--crash-dialog <файл>` (или `--crash-dialog=<файл>`).
/// `None` — это не процесс окна о крахе. Флаг без файла — тоже `None`:
/// показывать нечего, и такой запуск идёт обычным путём.
pub fn crash_dialog_request(args: &[String]) -> Option<CrashDialogRequest> {
    let eq = format!("{CRASH_DIALOG_FLAG}=");
    let mut path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == CRASH_DIALOG_FLAG {
            path = it.next().cloned();
            break;
        }
        if let Some(p) = a.strip_prefix(&eq) {
            path = Some(p.to_string());
            break;
        }
    }
    let path = path.filter(|p| !p.is_empty() && !p.starts_with("--"))?;
    Some(CrashDialogRequest {
        report_path: path,
        fatal: !args.iter().any(|a| a == CRASH_NONFATAL_FLAG),
    })
}

/// Аргументы процесса окна о крахе — обратная сторона
/// [`crash_dialog_request`], чтобы разбор и сборка не разошлись.
pub fn crash_dialog_argv(report_path: &str, fatal: bool) -> Vec<String> {
    let mut argv = vec![CRASH_DIALOG_FLAG.to_string(), report_path.to_string()];
    if !fatal {
        argv.push(CRASH_NONFATAL_FLAG.to_string());
    }
    argv
}

// ============================================================================
// Маршруты без Qt
// ============================================================================

/// Отработать маршрут, которому Qt не нужен. → код возврата процесса.
///
/// `None` — маршрут требует окна: главное окно либо одиночная вкладка,
/// которая теперь тот же раздел того же окна.
///
/// `--recover-kde` окна не открывает вовсе — и это не экономия, а условие
/// работы: его зовут, когда оформление уже сломано, и Qt в таком сеансе
/// может не подняться. Код возврата тот же, что у оригинала: 0 —
/// восстановили, 1 — не тот рабочий стол либо тема не применилась.
///
/// `out` — приёмник строк (в бинаре `println!`): строки идут ровно в том
/// порядке и теми словами, какими их печатал оригинал.
pub fn run_headless(args: &[String], out: &mut dyn FnMut(&str)) -> Option<i32> {
    let route = crate::app::route(args);
    // Сторож входа в Plasma (`--session-guard*`): его зовёт сценарий до
    // KWin — раньше всех маршрутов и без окна. Справку и версию он не
    // перехватывает: `--help` ничего не делает, только рассказывает.
    if !matches!(route, Route::Help | Route::Version) {
        if let Some(rc) = crate::session_guard::cli_route(args, out) {
            return Some(rc);
        }
    }
    match route {
        Route::Help => {
            help_lines().iter().for_each(|l| out(l));
            Some(0)
        }
        Route::Version => {
            out(&version_text());
            Some(0)
        }
        Route::ApplyDefaults => Some(crate::appdefaults_exec::apply_defaults_route_live(out)),
        Route::SanitizeGtk => Some(sanitize_gtk_route(out)),
        Route::SessionSplash => Some(session_splash_route()),
        Route::RecoverKde => Some(recover_kde_route(out)),
        Route::AeroshellCheck { login } => {
            Some(crate::system_de_exec::aeroshell_check_route_live(login, out))
        }
        Route::UpdateCheck { login } => Some(crate::update_guard::check_route_live(login, out)),
        Route::ConkySet => Some(crate::conky_sets::cli_route(args, out)),
        Route::Doctor => Some(crate::login_rescue::doctor_live(out)),
        Route::DayNight => Some(crate::day_night::cli_route(args, out)),
        Route::SingleTab { .. } | Route::MainWindow => None,
    }
}

/// `rexon --version`: имя, версия и коммит сборки — та же строка, что
/// пишет журнал при запуске (`applog::version_line`).
pub fn version_text() -> String {
    format!("Rexon {}", crate::applog::version_line())
}

/// `rexon --help`: что можно набрать в консоли. Имена команд и флагов —
/// буквально (их набирают руками), переводится только пояснение.
///
/// В справке — то, что пригодится человеку. Хуки, которые зовут юниты и
/// сценарии входа (`--apply-defaults`, `--session-splash`,
/// `--update-check`, `--aeroshell-check`, `--session-guard`,
/// `--session-guard-notice`), в ней
/// намеренно нет: руками их не запускают.
pub fn help_lines() -> Vec<String> {
    let row = |cmd: &str, what: String| format!("  {cmd:<32}{what}");
    // Страницы — из той же таблицы, что разбирает флаг: новая страница
    // ярлыка попадёт в справку сама.
    let pages: Vec<&str> = crate::app::PAGE_TARGETS.iter().map(|(p, _, _)| *p).collect();
    let page_flags = format!("{}{}", crate::app::PAGE_FLAG, pages.join("|"));
    vec![
        // TRANSLATORS: first line of `rexon --help`. "rexon [COMMAND]" is typed literally, keep it.
        tr("Usage: rexon [COMMAND]"),
        String::new(),
        // TRANSLATORS: `rexon --help`, heading of the list of commands that open the Rexon window.
        tr("Open the window (needs a desktop session):"),
        // TRANSLATORS: `rexon --help`, explains running `rexon` without arguments.
        row("rexon", tr("open Rexon")),
        // TRANSLATORS: `rexon --help`, explains the --page=install|backups|compare option.
        row(&page_flags, tr("open Rexon on this page")),
        // TRANSLATORS: `rexon --help`, explains the --tray option.
        row(crate::tray::START_HIDDEN_FLAG, tr("start hidden, with only the tray icon")),
        String::new(),
        // TRANSLATORS: `rexon --help`, heading of the list of commands that work in a text console.
        tr("Work without a window (also from a text console):"),
        // TRANSLATORS: `rexon --help`, explains `rexon doctor`.
        row(crate::login_rescue::DOCTOR_WORD, tr("find out why the login screen doesn't start (changes nothing)")),
        // TRANSLATORS: `rexon --help`, explains the --recover-kde option.
        row(crate::app::RECOVER_KDE_FLAG, tr("roll back a broken Plasma look to the standard one")),
        // TRANSLATORS: `rexon --help`, explains the --session-guard-restore option.
        row(crate::session_guard::RESTORE_FLAG, tr("bring back the Plasma look that Rexon set aside after a crash")),
        // TRANSLATORS: `rexon --help`, explains the --sanitize-gtk option.
        row("--sanitize-gtk", tr("fix links to deleted themes in GTK settings")),
        // TRANSLATORS: `rexon --help`, explains the --conky-set option; NAME is typed literally.
        row(&format!("{} NAME", crate::conky_sets::CLI_FLAG), tr("apply a saved set of Conky widgets")),
        // TRANSLATORS: `rexon --help`, explains the --version option.
        row("--version", tr("show the Rexon version")),
        // TRANSLATORS: `rexon --help`, explains the --help option.
        row("--help", tr("show this help")),
    ]
}

/// Можно ли этому процессу поднять окно: есть дисплей (`WAYLAND_DISPLAY`,
/// `DISPLAY`) либо явно выбрана платформа Qt без экрана
/// (`QT_QPA_PLATFORM=offscreen` у самопроверки). Без этого
/// `QGuiApplication` падает с `abort` («could not connect to display») —
/// дамп памяти вместо слов. Решение — [`crate::qt_app::qt_can_start`].
pub fn window_can_open(env: &dyn Fn(&str) -> Option<String>) -> bool {
    crate::qt_app::qt_can_start(
        env("WAYLAND_DISPLAY").as_deref(),
        env("DISPLAY").as_deref(),
        env("QT_QPA_PLATFORM").as_deref(),
    )
}

/// Что сказать, когда окно запросили там, где его показать негде (SSH,
/// текстовая консоль). Вместе с кодом возврата 1 — вместо `abort`.
pub fn no_display_lines() -> Vec<String> {
    vec![
        // TRANSLATORS: printed when `rexon` is run where no window can be shown (SSH, text console).
        tr("Rexon can't open its window here: there is no desktop session (WAYLAND_DISPLAY and DISPLAY are not set)."),
        // TRANSLATORS: second line of the "no desktop session" message. Commands are typed literally.
        tr("Open Rexon from your desktop. In a text console these work: rexon doctor, rexon --help."),
    ]
}

/// Маршрут `--sanitize-gtk` — порт ветки `rexon/app.py:640`.
///
/// Починить ссылки на отсутствующие темы иконок/курсоров/цветов в
/// `~/.config/gtk-{3,4}.0/settings.ini`, `kdeglobals` и `kcminputrc`. Без
/// этого GTK-приложения (Firefox, Nautilus) уходят в петлю поиска иконок у
/// темы, которой на диске уже нет, и падают с SIGSEGV после применения
/// Global Theme.
///
/// Решения — целиком [`crate::gtk_sanitizer`], исполнение — живой
/// санитайзер установщика, текст отчёта — `gtk_sanitizer::verbose_report`
/// (тот же, что печатает Python: `print(report, end="")`, поэтому отчёт
/// разбирается на строки, а не печатается одним куском с лишним переводом).
pub fn sanitize_gtk_route(out: &mut dyn FnMut(&str)) -> i32 {
    let changes = crate::appearance_installer::live_gtk_sanitize(&home());
    for line in crate::gtk_sanitizer::verbose_report(&changes).lines() {
        out(line);
    }
    for line in gtk_note_lines(&crate::appearance_installer::live_gtk_notes(&home())) {
        out(&line);
    }
    0
}

/// Санитайзер GTK при старте окна — порт блока «Авто-санитайзер GTK-конфигов
/// перед стартом» из `rexon/app.py`.
///
/// Тот же живой санитайзер, что у маршрута `--sanitize-gtk` выше, но
/// молчаливый, если чинить нечего: при старте отчёт нужен только о
/// сделанном. Битые ссылки на темы в gtk-{3,4}.0 роняют чужие
/// GTK-приложения, поэтому чиним до первого окна.
pub fn startup_gtk_sanitize(out: &mut dyn FnMut(&str)) {
    let home = home();
    for line in startup_gtk_report(&crate::appearance_installer::live_gtk_sanitize(&home)) {
        out(&line);
    }
    for line in gtk_note_lines(&crate::appearance_installer::live_gtk_notes(&home)) {
        out(&line);
    }
}

/// Строки о ссылках, которые санитайзер заметил, но НЕ правил (звуковая
/// тема, которой нет: выбор человека не подменяется). Пусто — замечаний нет.
fn gtk_note_lines(notes: &[String]) -> Vec<String> {
    notes.iter().map(|n| format!("⚠️ {n}")).collect()
}

/// Строки стартового отчёта санитайзера; пусто — чинить было нечего.
fn startup_gtk_report(changes: &[String]) -> Vec<String> {
    if changes.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![ntr(
        "🛡️ GTK protection: fixed %n broken link:",
        "🛡️ GTK protection: fixed %n broken links:",
        changes.len() as u64,
    )];
    lines.extend(changes.iter().map(|c| format!("   • {c}")));
    lines
}

/// Первое снятие эталона защиты приложений при старте окна.
///
/// Раздел «Защита приложений по умолчанию» включён по умолчанию, но до
/// первого эталона Rexon не помнит НИЧЕГО и подмены не заметит. Покупателю
/// для этого нажимать нечего не должно быть: эталон снимается сам, молча для
/// глаз — ни окна, ни вопроса.
///
/// ⚠️ ОТДЕЛЬНЫМ ПОТОКОМ, а не в пути запуска окна. Снятие спрашивает систему
/// о приложении по умолчанию для каждого типа — это `xdg-mime` и `gsettings`,
/// то есть с десяток подпроцессов. ⭐Замер на стенде (приватный профиль,
/// xvfb, эта машина): **573 мс** на чистом профиле — и это ещё быстрый
/// случай, на медленной машине или при тормозящем `xdg-mime` больше.
/// В пути запуска это дало бы висящее окно без единого слова —
/// ровно тот дефект, который в продукте уже ловили на ожидании демона обоев.
/// Того же вида зацепка, что у объявления в мост у входа окна.
///
/// Молча для глаз — но НЕ для журнала: разбирающий потом должен видеть, что
/// эталон сняли сами, сколько запомнили и сколько это заняло.
pub fn startup_defaults_snapshot(mut out: Box<dyn FnMut(&str) + Send>) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = crate::system_de_exec::startup_defaults_snapshot_live();
        let elapsed = started.elapsed().as_millis();
        for line in startup_snapshot_report(&outcome, elapsed) {
            out(&line);
        }
    });
}

/// Строки журнала о первом снятии эталона; пусто — снимать было нечего.
fn startup_snapshot_report(
    outcome: &Option<Result<usize, String>>,
    elapsed_ms: u128,
) -> Vec<String> {
    match outcome {
        None => Vec::new(),
        Some(Ok(count)) => vec![trf(
            "🛡️ App protection: baseline taken on first launch (remembered: %1, in %2 ms)",
            &[count, &elapsed_ms],
        )],
        Some(Err(e)) => vec![trf(
            "⚠️ App protection: couldn't take the baseline on first launch: %1",
            &[e],
        )],
    }
}

/// Поля объявления Rexon в мост — порт `announce_self` (`rexon/rexon_bridge.py`):
/// версия всегда, окружение и его человеческое имя — если известны.
fn announce_fields(version: &str, current_de: &str, display: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("rexon_version".into(), version.into());
    if !current_de.is_empty() {
        fields.insert("current_de".into(), current_de.into());
    }
    if !display.is_empty() {
        fields.insert("current_de_display".into(), display.into());
    }
    fields
}

/// Объявить Rexon в мост при старте окна (`rexon_installed`, версия,
/// окружение) — то, что окно на Python делает `announce_self`.
///
/// Без объявления флаг `rexon_installed` в `bridge.json` не выставляется, и
/// первая же проверка внешнего приложения «Rexon вообще стоит?» отвечает
/// «нет». Дорогих проб нет: только окружение и имя из ядра.
pub fn announce_to_bridge() -> std::io::Result<()> {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let de = crate::appdefaults::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default();
    let display = if de.is_empty() { String::new() } else { crate::screenshots::display_name(&de) };
    let fields = announce_fields(&crate::window_menu::version_string(), &de, &display);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let bridge = crate::rexon_bridge::RexonBridge::new_default();
    bridge.ensure_dirs()?;
    bridge.update_rexon_state(fields, &crate::engine_update::now_iso(now.as_secs() as i64))
}

/// Маршрут `--recover-kde` — порт ветки `rexon/app.py:682` и девятнадцати
/// её помощников (376 строк).
///
/// Аварийный откат оформления: вернуть Global Theme из резервной копии,
/// снять переопределения шрифтов и эффекты KWin от форк-тем, вернуть
/// загрузочную заставку и экран входа к стоку дистрибутива, обои — к
/// стандартным. Ни одной строки Qt на пути: маршрут обязан работать в
/// сеансе, где Qt уже не поднимается.
///
/// Решения и тексты — [`crate::app`], порядок шагов и походы наружу —
/// [`crate::system_exec`]; здесь только заведение живого исполнителя и код
/// возврата.
pub fn recover_kde_route(out: &mut dyn FnMut(&str)) -> i32 {
    crate::system_exec::live().recover_kde(out)
}

/// Маршрут `--session-splash` — порт ветки `rexon/app.py:679`.
///
/// Autostart-`.desktop` дёргает его при входе в сессию: проиграть выбранный
/// webm полноэкранно и выбрать ролик на следующий вход. Qt здесь не
/// поднимается вовсе — ни одной строки Qt на пути.
///
/// Код возврата всегда 0, и это НЕ небрежность: так решает ядро
/// (`session_splash::Splash::main`) — неудача заставки не смеет мешать
/// входу в сессию, она уходит предупреждением в журнал.
pub fn session_splash_route() -> i32 {
    let home = home();
    let repo = repo_root();
    // Интерпретатор — не `sys.executable` оригинала: в Rust-процессе его нет.
    // Нужен только откату строки автозапуска на Python-вход.
    let mut io = crate::splash_exec::LiveSplashIo::new(
        &home,
        crate::installer::PYTHON_FALLBACK_EXE,
    );
    let mut splash = crate::session_splash::Splash::new(&mut io, &home);
    splash.main(&repo) as i32
}

// ============================================================================
// Сообщения Qt и QML в журнал Rexon
// ============================================================================

/// Ставился ли уже логгер сообщений Qt (идемпотентность как у оригинала).
static QT_LOGGER_INSTALLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Завести сообщения Qt и QML в журнал Rexon — порт
/// `ui_base.install_qt_message_logger` (59 строк).
///
/// Ошибка в делегате списка, не найденный QML-модуль, TypeError в привязке —
/// всё это диагностика того же уровня, что остальные записи, и обязана
/// лежать в том же файле. У Rust-бинаря не попадало НИ ОДНО сообщение QML, а
/// пустой журнал читается как «QML молчит, ошибок нет».
///
/// ⛔ ОБРАБОТЧИКА СООБЩЕНИЙ СВОИМ КОДОМ ЗДЕСЬ НЕТ. Qt печатает не только из
/// главного потока (QQmlThread компилирует QML и сам ругается на него), а
/// разбирать сообщение в чужом потоке — тот самый капкан, из-за которого в
/// Python-части маршрут сделан переменными окружения. Печатает штатный
/// обработчик Qt на C++, мы лишь задаём ему формат и направление.
///
/// Ставить ДО создания `QGuiApplication` — тогда видно и сообщения самой
/// инициализации Qt. Отвечает `true`, если дескриптор 2 заведён в файл
/// журнала.
pub fn install_qt_message_logger() -> bool {
    use std::sync::atomic::Ordering;
    if QT_LOGGER_INSTALLED.swap(true, Ordering::SeqCst) {
        return false;
    }
    // `setdefault`, а не `set`: явно заданный человеком формат или
    // направление — его выбор, и перебивать его нельзя.
    for (name, value) in crate::ui_base::qt_logging_env() {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, value);
        }
    }
    redirect_stderr_to_log()
}

/// Завести дескриптор 2 в файл журнала. `true` — завели.
///
/// Правило «перехватывать или нет» живёт в ядре
/// ([`crate::ui_base::should_redirect_stderr`]), здесь только наблюдения за
/// дескриптором: терминал, файл и труба — явный выбор запустившего, его не
/// перехватываем, иначе разработчик перестанет видеть сообщения там, где
/// ожидает. Перехватываем запуск без консоли (ярлык «Пуск», systemd-юнит),
/// где stderr — сокет journald либо `/dev/null`.
fn redirect_stderr_to_log() -> bool {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(2, &mut st) } != 0 {
        return false;
    }
    let mode = st.st_mode;
    let is_tty = unsafe { libc::isatty(2) } == 1;
    if !crate::ui_base::should_redirect_stderr(
        is_tty,
        mode & libc::S_IFMT == libc::S_IFSOCK,
        mode & libc::S_IFMT == libc::S_IFCHR,
        is_tty,
    ) {
        return false;
    }

    // Файл и ротация — у общего журнала (`crate::applog`): дескриптор 2
    // уходит в тот же файл, а исходный поток ошибок (журнал systemd)
    // остаётся за копиями строк Rexon. Прежде здесь был свой `dup2` в
    // `~/.config/rexon/rexon.log` без предела роста и без копии в systemd.
    crate::applog::init();
    crate::applog::redirect_stderr()
}

// ============================================================================
// Надписи на СОБСТВЕННЫХ кнопках Qt — на языке Rexon
// ============================================================================
//
// Кнопки, которые рисует сама Qt («OK», «Cancel», «Show Details…»,
// «Apply», меню правки в полях ввода), переводятся только с загруженным
// каталогом qtbase — без него посреди русского окна попадаются английские
// надписи. Каталог берём у самой Qt, а не носим свой.
//
// ЯЗЫК — ЯЗЫК REXON, а не системы. Раньше каталог выбирался по
// `QLocale::system()` с откатом на русский: при английском Rexon на русской
// системе стандартный диалог говорил «Отмена» посреди английского окна, а
// на системе без каталога Qt (скажем, японской) кнопки становились
// русскими. Теперь язык ровно тот, что выбрало ядро для своих строк
// ([`crate::i18n::current_language`]); у английского (исходного языка Qt)
// каталога нет вовсе, и откат — тоже на английский: без каталога Qt сама
// рисует английские надписи.
//
// В ядре — РЕШЕНИЕ (какие файлы, в каком порядке, для какого языка), в
// Qt-слое — исполнение (`QTranslator`, `QLibraryInfo`): обёрток ни того, ни
// другого в cxx-qt-lib 0.9 нет вовсе. Шим отдаёт каталог переводов Qt и
// грузит ровно те файлы, которые назвало ядро.

/// Каталоги переводов самой Qt, в порядке загрузки.
///
/// `qtbase` — кнопки диалогов и меню правки, то есть всё, что покупатель
/// видит; `qt` — исторический сводный каталог старых сборок Qt, он есть не
/// везде и грузится вторым.
pub const QT_TRANSLATION_CATALOGS: &[&str] = &["qtbase", "qt"];

/// Разделитель имени каталога и кода языка (`qtbase_ru.qm`).
pub const QT_TRANSLATION_SEPARATOR: &str = "_";

/// Имена файлов (без `.qm`) каталога `catalog` для языка Rexon `language`,
/// по убыванию предпочтения: `pt_BR` → `qtbase_pt_BR`, `qtbase_pt`.
///
/// Английский — исходный язык Qt: для него пусто, никакого каталога.
/// `language` — код из [`crate::i18n::current_language`] (`"ru"`,
/// `"pt_BR"`, `"en"`); кодировка и модификатор (`.UTF-8`, `@euro`), если
/// вдруг приехали, отбрасываются.
pub fn qt_translation_candidates(catalog: &str, language: &str) -> Vec<String> {
    let code = language.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
    let base = code.split('_').next().unwrap_or("").to_string();
    if base.is_empty() || base == crate::i18n::SOURCE_LANGUAGE || code == "C" || code == "POSIX" {
        return Vec::new();
    }
    let mut out = vec![format!("{catalog}{QT_TRANSLATION_SEPARATOR}{code}")];
    if base != code {
        out.push(format!("{catalog}{QT_TRANSLATION_SEPARATOR}{base}"));
    }
    out
}

/// Какие файлы `.qm` самой Qt включить для языка Rexon `language`: для
/// каждого из [`QT_TRANSLATION_CATALOGS`] — первый найденный кандидат в
/// каталоге переводов Qt `dir`.
///
/// Пусто — английский: либо Rexon по-английски, либо для его языка у Qt
/// нет каталога. На другой язык (прежде — на русский) НЕ откатываемся:
/// кнопки чужого языка посреди окна хуже английских, которые Qt рисует
/// сама.
pub fn qt_translation_files(
    language: &str,
    dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    QT_TRANSLATION_CATALOGS
        .iter()
        .filter_map(|catalog| {
            qt_translation_candidates(catalog, language)
                .into_iter()
                .map(|name| dir.join(format!("{name}.qm")))
                .find(|path| exists(path))
        })
        .collect()
}

/// Переменная окружения, по которой язык стандартных кнопок выбирает
/// ТЕМА ПЛАТФОРМЫ (KDE Plasma: `KDEPlasmaPlatformTheme6`).
///
/// ⭐ Поймано на стенде 30.09.2026: под `XDG_CURRENT_DESKTOP=KDE` кнопки
/// «Yes/No» стандартного диалога QML рисует не qtbase, а тема Plasma — она
/// переводит их своим каталогом KF6 (KLocalizedString) по языку СИСТЕМЫ.
/// Без каталога qtbase на русской системе при английском Rexon диалог всё
/// равно говорил «Да/Нет». KLocalizedString слушает `LANGUAGE`, поэтому
/// окно до создания приложения ставит её в язык Rexon — и тема Plasma
/// (как и любая другая, что слушает gettext) идёт за Rexon.
pub const QT_UI_LANGUAGE_ENV: &str = "LANGUAGE";

/// Пробный запуск перед перезапуском: бинарь окна с этой переменной
/// выходит с нулём первой строкой `main`. Вместе с `LD_BIND_NOW=1` это
/// проверка «новая копия вообще загружается» — все символы библиотек
/// привязываются до `main`, и смесь версий Qt видна сразу.
pub const LINK_PROBE_ENV: &str = "REXON_LINK_PROBE";

/// Значение [`QT_UI_LANGUAGE_ENV`] для языка Rexon `language` (код из
/// [`crate::i18n::current_language`]): сам код без кодировки; пусто или
/// `C` — английский, исходный язык.
pub fn qt_ui_language_env(language: &str) -> String {
    let code = language.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
    if code.is_empty() || code == "C" || code == "POSIX" {
        crate::i18n::SOURCE_LANGUAGE.to_string()
    } else {
        code
    }
}

// ============================================================================
// Строки окна в терминал
// ============================================================================

/// Что сказать в терминал о значке в лотке при старте окна: `icon_shown` —
/// значок встал, `starts_hidden` — просили запуск свёрнутым.
pub fn tray_status_lines(icon_shown: bool, starts_hidden: bool) -> Vec<String> {
    let status = match (icon_shown, starts_hidden) {
        (true, false) => tr("🔔 Tray: icon added"),
        (true, true) => tr("🔔 Tray: icon added, starting minimized"),
        (false, false) => tr("🔔 Tray: no icon"),
        (false, true) => tr("🔔 Tray: no icon, starting minimized"),
    };
    let mut lines = vec![status];
    if starts_hidden && !icon_shown {
        // Просили свернуться, но значка нет — окно единственная дверь в
        // программу, и прятать его нельзя (правило #13).
        lines.push(tr("⚠️ Starting minimized was canceled: there's no tray icon"));
    }
    lines
}

/// Сколько проигрывателей остановлено перед выходом (печатается, только
/// если их было больше нуля).
pub fn players_stopped_line(players: i64) -> String {
    trf("⏹️ Media players stopped before exit: %1", &[&players])
}

// ============================================================================
// Наблюдения, которых у чистого ядра нет
// ============================================================================

/// Домашний каталог (`os.path.expanduser("~")` оригинала).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// Корень дерева разработки: там ядро ищет собранный
/// `rexon-splash-overlay`.
///
/// Величина не выдумывается, а выводится из пути своего же бинаря
/// (`…/rust/target/<профиль>/rexon-main-window`); не вывелась — пустая
/// строка, и ядро уйдёт на поиск по `PATH`, как оно и умеет.
///
/// Открыта крейту вкладок: по тому же корню Rust-окно находит пакет
/// `rexon/` (вендорный интерфейс вкладки обоев), своей второй копии
/// вывода там нет.
pub fn repo_root() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return String::new();
    };
    repo_root_of(&exe)
}

/// Тот же вывод, но вокруг ЧУЖОГО файла: первый каталог выше него, в
/// котором лежит `rust/Cargo.toml`. Пусто — файл не в дереве.
///
/// ⭐ Отдельная дверь нужна не ради красоты. У входа через Python своего
/// бинаря нет: `current_exe` там — `/usr/bin/python3`, и от него дерево не
/// выводится никогда. Мост (`rexon-py`) держал ради этого свою копию
/// правила — вторую правду о том, что считать деревом разработчика; разойдись
/// они, и решение «предлагать ли мастер первого запуска» стало бы разным у
/// окна и у Python. Правило живёт здесь, а [`repo_root`] — частный случай
/// «файл = свой бинарь» (правило #12).
pub fn repo_root_of(path: &std::path::Path) -> String {
    for dir in path.ancestors() {
        if dir.join("rust").join("Cargo.toml").is_file() {
            return dir.to_string_lossy().into_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Корень вокруг ЧУЖОГО файла — то, ради чего дверь и заведена.
    ///
    /// Дерево лепится настоящее: правило смотрит наличие файла
    /// `rust/Cargo.toml`, подставным наблюдением его не подменить.
    #[test]
    fn repo_root_of_foreign_path() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("rexon-repo-root-{nanos}"));
        let root = base.join("Загрузки").join("rexon_v5");
        std::fs::create_dir_all(root.join("rust")).expect("каталог rust создан");
        std::fs::write(root.join("rust").join("Cargo.toml"), "[workspace]\n")
            .expect("манифест дерева записан");
        std::fs::create_dir_all(root.join("rexon")).expect("каталог пакета создан");
        let module = root.join("rexon").join("app.py");
        std::fs::write(&module, "").expect("модуль записан");

        // Вход через Python: своего бинаря нет (`current_exe` — интерпретатор),
        // и дерево выводится вот от этого модуля.
        assert_eq!(repo_root_of(&module), root.to_string_lossy());
        // Каталог самого дерева тоже внутри себя.
        assert_eq!(repo_root_of(&root), root.to_string_lossy());

        // Вне дерева — пусто, и это не небрежность: так у покупателя стоит
        // установленный пакет, и «а вдруг дерево» выдумывать нельзя.
        let outside = base.join("usr").join("lib").join("rexon").join("app.py");
        std::fs::create_dir_all(outside.parent().expect("родитель есть"))
            .expect("каталог установки создан");
        std::fs::write(&outside, "").expect("файл записан");
        assert_eq!(repo_root_of(&outside), "");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crash_dialog_flag_forms_and_roundtrip() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            crash_dialog_request(&a(&["--crash-dialog", "/c/crash_1.log"])),
            Some(CrashDialogRequest { report_path: "/c/crash_1.log".into(), fatal: true })
        );
        assert_eq!(
            crash_dialog_request(&a(&["--crash-nonfatal", "--crash-dialog=/c/Отчёт 1.log"])),
            Some(CrashDialogRequest { report_path: "/c/Отчёт 1.log".into(), fatal: false })
        );
        // Флаг без файла — не процесс окна: показывать нечего.
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog"])), None);
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog", "--tray"])), None);
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog="])), None);
        assert_eq!(crash_dialog_request(&a(&["--section=appearance"])), None);
        for fatal in [true, false] {
            let argv = crash_dialog_argv("/r/crash_2.log", fatal);
            assert_eq!(
                crash_dialog_request(&argv),
                Some(CrashDialogRequest { report_path: "/r/crash_2.log".into(), fatal })
            );
        }
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Чинить было нечего — при старте молчим; починили — число и список.
    #[test]
    fn startup_gtk_report_speaks_only_about_repairs() {
        english();
        assert!(startup_gtk_report(&[]).is_empty());
        let lines = startup_gtk_report(&["gtk-3.0: icon theme".to_string()]);
        assert_eq!(lines[0], "🛡️ GTK protection: fixed 1 broken link:");
        assert_eq!(lines[1], "   • gtk-3.0: icon theme");
    }

    /// Замечание о неправленой ссылке — отдельной строкой, без слова «fixed».
    #[test]
    fn gtk_note_lines_do_not_claim_a_fix() {
        assert!(gtk_note_lines(&[]).is_empty());
        let lines = gtk_note_lines(&["gtk-3.0/settings.ini: x".to_string()]);
        assert_eq!(lines, ["⚠️ gtk-3.0/settings.ini: x"]);
    }

    /// Поля объявления — как у `announce_self`: версия всегда, окружение и
    /// его имя только если известны (пустые в мост не пишутся).
    #[test]
    fn announce_fields_skip_unknown_desktop() {
        let full = announce_fields("0.0.1", "kde", "KDE Plasma");
        let keys: Vec<&str> = full.keys().map(String::as_str).collect();
        assert_eq!(keys, ["rexon_version", "current_de", "current_de_display"]);
        let bare = announce_fields("0.0.1", "", "");
        assert_eq!(bare.keys().map(String::as_str).collect::<Vec<_>>(), ["rexon_version"]);
    }

    #[test]
    fn каждый_флаг_одиночной_вкладки_ведёт_в_раздел() {
        for (flag, module) in crate::app::SINGLE_TAB_FLAGS {
            let (section, _sub) = single_tab_section(module)
                .unwrap_or_else(|| panic!("у флага {flag} нет раздела"));
            assert!(
                crate::qt_main::SECTIONS.contains(&section),
                "раздел {section} флага {flag} не из состава окна"
            );
        }
    }

    #[test]
    fn у_разделов_нет_лишних_записей() {
        assert_eq!(SINGLE_TAB_SECTIONS.len(), crate::app::SINGLE_TAB_FLAGS.len());
        for (module, _, _) in SINGLE_TAB_SECTIONS {
            assert!(
                crate::app::SINGLE_TAB_FLAGS.iter().any(|(_, m)| m == module),
                "модуль {module} не соответствует ни одному флагу"
            );
        }
    }

    #[test]
    fn section_сильнее_флага_вкладки_и_ярлыка() {
        assert_eq!(
            start_section(&argv(&["--section=steam", "--settings-qt", "--page=install"])),
            Some(("steam".into(), String::new()))
        );
    }

    #[test]
    fn флаг_вкладки_сильнее_ярлыка() {
        assert_eq!(
            start_section(&argv(&["--compare-qt", "--page=backups"])),
            Some(("desktops".into(), "compare".into()))
        );
    }

    #[test]
    fn ярлык_переводится_как_и_прежде() {
        assert_eq!(
            start_section(&argv(&["--page=backups"])),
            Some(("settings".into(), String::new()))
        );
        assert_eq!(start_section(&argv(&[])), None);
    }

    /// Пустой `--section=` — это не «раздел с пустым именем», а опечатка:
    /// окно обязано открыться на разделе по умолчанию, а не на пустоте.
    #[test]
    fn пустой_section_не_считается_разделом() {
        assert_eq!(start_section(&argv(&["--section="])), None);
        assert_eq!(
            start_section(&argv(&["--section=  ", "--page=install"])),
            Some(("desktops".into(), String::new()))
        );
    }

    #[test]
    fn маршруты_окна_кода_возврата_не_дают() {
        let mut said: Vec<String> = Vec::new();
        let mut out = |s: &str| said.push(s.to_string());
        assert_eq!(run_headless(&argv(&[]), &mut out), None);
        assert_eq!(run_headless(&argv(&["--settings-qt"]), &mut out), None);
        assert!(said.is_empty(), "маршруты окна ничего не печатают");
    }

    /// `rexon --help` и `--version` без дисплея: до 30.10.2026 оба
    /// проваливались в главное окно, и Qt падал с `abort` вместо справки.
    /// Теперь это маршруты без окна с кодом 0 — и они сильнее любого
    /// другого флага (`--recover-kde --help` рассказывает, а не откатывает).
    #[test]
    fn справка_и_версия_без_окна_и_с_нулём() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for args in [&["--help"][..], &["-h"], &["help"], &["--recover-kde", "--help"], &["--session-guard", "-h"]] {
            assert_eq!(crate::app::route(&argv(args)), Route::Help, "{args:?}");
            let mut said: Vec<String> = Vec::new();
            assert_eq!(run_headless(&argv(args), &mut |s| said.push(s.to_string())), Some(0), "{args:?}");
            assert_eq!(said, help_lines(), "{args:?}");
        }
        for args in [&["--version"][..], &["-V"], &["--apply-defaults", "--version"]] {
            assert_eq!(crate::app::route(&argv(args)), Route::Version, "{args:?}");
            let mut said: Vec<String> = Vec::new();
            assert_eq!(run_headless(&argv(args), &mut |s| said.push(s.to_string())), Some(0), "{args:?}");
            assert_eq!(said, vec![version_text()]);
        }
        assert!(version_text().starts_with("Rexon "), "{}", version_text());
        // Слово `help` — только первым: набор Conky может так называться.
        assert_eq!(crate::app::route(&argv(&["--conky-set", "help"])), Route::ConkySet);
        // В справке есть каждая консольная команда, которую стоит знать.
        let text = help_lines().join("\n");
        for cmd in ["doctor", "--recover-kde", "--session-guard-restore", "--sanitize-gtk", "--conky-set", "--version", "--help", "--page=", "--tray"] {
            assert!(text.contains(cmd), "в справке нет {cmd}:\n{text}");
        }
    }

    /// Окно без дисплея: понятные строки вместо `abort` Qt. Самопроверка с
    /// `QT_QPA_PLATFORM=offscreen` дисплея не требует.
    #[test]
    fn окно_без_дисплея_не_поднимается() {
        let env_of = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert!(!window_can_open(&env_of(&[])));
        assert!(!window_can_open(&env_of(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])));
        assert!(!window_can_open(&env_of(&[("QT_QPA_PLATFORM", "wayland;xcb")])));
        assert!(window_can_open(&env_of(&[("WAYLAND_DISPLAY", "wayland-0")])));
        assert!(window_can_open(&env_of(&[("DISPLAY", ":0")])));
        assert!(window_can_open(&env_of(&[("QT_QPA_PLATFORM", "offscreen")])));
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let lines = no_display_lines();
        assert!(lines.iter().any(|l| l.contains("rexon doctor")), "{lines:?}");
    }

    /// `--recover-kde` — маршрут с кодом возврата, а не окно. Сам прогон
    /// здесь не делается НАМЕРЕННО: маршрут меняет живую машину (pkexec,
    /// plasma-apply-*, перезапуск plasmashell). Его порядок шагов и слова
    /// проверяются на подставном вводе-выводе в
    /// `tests/system_exec_parity.rs`, где живой машины нет вовсе.
    #[test]
    fn recover_kde_разбирается_маршрутом_а_не_окном() {
        assert_eq!(
            crate::app::route(&argv(&["--recover-kde"])),
            Route::RecoverKde
        );
    }

    /// Каталог переводов Qt этой сборки в миниатюре: русский, немецкий,
    /// бразильский португальский — и английский, который грузить нельзя.
    const QT_DIR: &str = "/qt/translations";
    fn qt_has(files: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| {
            p.parent() == Some(Path::new(QT_DIR))
                && files.iter().any(|f| p.file_name().and_then(|n| n.to_str()) == Some(*f))
        }
    }
    const SHIPPED: &[&str] =
        &["qtbase_ru.qm", "qt_ru.qm", "qtbase_de.qm", "qtbase_en.qm", "qt_en.qm", "qtbase_pt_BR.qm"];

    fn qm(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| Path::new(QT_DIR).join(n)).collect()
    }

    #[test]
    fn кандидаты_каталога_qt_по_языку_rexon() {
        assert_eq!(qt_translation_candidates("qtbase", "ru"), vec!["qtbase_ru"]);
        assert_eq!(qt_translation_candidates("qtbase", "pt_BR"), vec!["qtbase_pt_BR", "qtbase_pt"]);
        assert_eq!(qt_translation_candidates("qt", "de_DE.UTF-8"), vec!["qt_de_DE", "qt_de"]);
        for english in ["en", "en_GB", "", "C", "POSIX"] {
            assert!(qt_translation_candidates("qtbase", english).is_empty(), "{english:?}");
        }
    }

    /// Какой `.qm` самой Qt выбирается: язык решает Rexon, а не система.
    #[test]
    fn файлы_qt_выбираются_по_языку_rexon() {
        let exists = qt_has(SHIPPED);
        let dir = Path::new(QT_DIR);
        // Rexon по-русски — оба каталога.
        assert_eq!(qt_translation_files("ru", dir, &exists), qm(&["qtbase_ru.qm", "qt_ru.qm"]));
        // По-немецки — только то, что есть (`qt_de` нет).
        assert_eq!(qt_translation_files("de", dir, &exists), qm(&["qtbase_de.qm"]));
        assert_eq!(qt_translation_files("pt_BR", dir, &exists), qm(&["qtbase_pt_BR.qm"]));
        // По-английски — НИЧЕГО, хотя `qtbase_en.qm` на диске есть.
        assert!(qt_translation_files("en", dir, &exists).is_empty());
        // Языка у Qt нет — откат на английский (пусто), а НЕ на русский.
        assert!(qt_translation_files("ja", dir, &exists).is_empty());
        assert!(qt_translation_files("uk", dir, &exists).is_empty());
    }

    /// Сквозная сверка: окружение ▸ язык Rexon (как его выбирает
    /// `i18n::activate`) ▸ файлы Qt. Каталог Rexon в этой сборке есть только
    /// у русского.
    fn qt_files_for(setting: Option<&str>, env: &[(&str, &str)]) -> Vec<PathBuf> {
        let env: Vec<(String, String)> =
            env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let lookup = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let candidates = crate::i18n::language_candidates(setting, &lookup);
        let dirs = vec![PathBuf::from("/rexon/locale")];
        let rexon_has = |p: &Path| p == crate::i18n::catalog_file(Path::new("/rexon/locale"), "ru", "mo");
        let language = crate::i18n::resolve(&candidates, &dirs, &rexon_has)
            .map(|(lang, _)| lang)
            .unwrap_or_else(|| crate::i18n::SOURCE_LANGUAGE.to_string());
        qt_translation_files(&language, Path::new(QT_DIR), &qt_has(SHIPPED))
    }

    /// Тема платформы (KDE) получает язык Rexon через `LANGUAGE`.
    #[test]
    fn язык_темы_платформы_равен_языку_rexon() {
        assert_eq!(QT_UI_LANGUAGE_ENV, "LANGUAGE");
        assert_eq!(qt_ui_language_env("en"), "en");
        assert_eq!(qt_ui_language_env("ru"), "ru");
        assert_eq!(qt_ui_language_env("pt_BR"), "pt_BR");
        assert_eq!(qt_ui_language_env("de_DE.UTF-8"), "de_DE");
        assert_eq!(qt_ui_language_env(""), "en");
        assert_eq!(qt_ui_language_env("C"), "en");
    }

    #[test]
    fn английский_rexon_на_русской_системе_без_каталога_qt() {
        let ru_system = [("LANG", "ru_RU.UTF-8")];
        // Система русская, Rexon по-русски — русские кнопки Qt.
        assert_eq!(qt_files_for(None, &ru_system), qm(&["qtbase_ru.qm", "qt_ru.qm"]));
        // `LANGUAGE=en` на русской системе — английские кнопки.
        assert!(qt_files_for(None, &[("LANGUAGE", "en"), ("LANG", "ru_RU.UTF-8")]).is_empty());
        // Настройка Rexon «английский» на русской системе — то же.
        assert!(qt_files_for(Some("en"), &ru_system).is_empty());
        // Язык, для которого у Rexon нет каталога (японская система): Rexon
        // говорит по-английски — и кнопки Qt тоже, а не русские.
        assert!(qt_files_for(None, &[("LANG", "ja_JP.UTF-8")]).is_empty());
        // Немецкий: у Qt каталог есть, у Rexon (в этой сборке) — нет. Кнопки
        // идут за Rexon, иначе посреди английского окна стояло бы «Abbrechen».
        assert!(qt_files_for(Some("de"), &ru_system).is_empty());
    }

    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn cyrillic(s: &str) -> bool {
        s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
    }

    #[test]
    fn english_terminal_lines_have_no_russian() {
        english();
        let mut all = Vec::new();
        all.extend(startup_gtk_report(&["gtk-3.0: icon theme".into(), "kdeglobals: x".into()]));
        all.extend(startup_snapshot_report(&Some(Ok(7)), 142));
        all.extend(startup_snapshot_report(&Some(Err("disk full".into())), 3));
        for icon in [true, false] {
            for hidden in [true, false] {
                all.extend(tray_status_lines(icon, hidden));
            }
        }
        all.push(players_stopped_line(2));
        for line in &all {
            assert!(!cyrillic(line), "русская буква в английском тексте: {line}");
        }
        assert_eq!(all[0], "🛡️ GTK protection: fixed 2 broken links:");
        assert_eq!(
            startup_gtk_report(&["gtk-3.0: icon theme".into()])[0],
            "🛡️ GTK protection: fixed 1 broken link:"
        );
        assert_eq!(
            startup_snapshot_report(&Some(Ok(7)), 142)[0],
            "🛡️ App protection: baseline taken on first launch (remembered: 7, in 142 ms)"
        );
        assert_eq!(
            tray_status_lines(false, true),
            vec![
                "🔔 Tray: no icon, starting minimized".to_string(),
                "⚠️ Starting minimized was canceled: there's no tray icon".to_string(),
            ]
        );
        assert_eq!(tray_status_lines(true, false), vec!["🔔 Tray: icon added".to_string()]);
        assert_eq!(players_stopped_line(2), "⏹️ Media players stopped before exit: 2");
    }

    /// Логгер идемпотентен: второй вызов ничего не делает (иначе дескриптор
    /// 2 заводился бы в журнал по кругу на каждой точке входа).
    #[test]
    fn логгер_ставится_один_раз() {
        install_qt_message_logger();
        assert!(!install_qt_message_logger());
    }

    /// Журнал первого снятия эталона: молчит, когда снимать нечего, и
    /// называет число запомненного, когда сняли.
    #[test]
    fn startup_snapshot_report_lines() {
        english();
        assert!(startup_snapshot_report(&None, 0).is_empty());
        let ok = startup_snapshot_report(&Some(Ok(7)), 142);
        assert_eq!(ok.len(), 1);
        assert!(ok[0].contains("remembered: 7"), "{}", ok[0]);
        assert!(ok[0].contains("142 ms"), "{}", ok[0]);
        let bad = startup_snapshot_report(&Some(Err("нет места".to_string())), 3);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("нет места"), "{}", bad[0]);
    }
}
