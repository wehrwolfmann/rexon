//! Сверки живого исполнителя маршрута `--apply-defaults`
//! ([`rexon_core::appdefaults_exec`]).
//!
//! ЧЕМ ЭТО ПРОВЕРЯЕТСЯ. Исполнитель гоняется на ПОДСТАВНОМ вводе-выводе
//! ([`FakeIo`]) с подставными `$HOME`/`$XDG_CONFIG_HOME`/`$XDG_DATA_HOME`:
//! ни один файл живой машины не читается и не пишется, ни один подпроцесс не
//! запускается. Ровно ради этого исполнитель и разделён на «решение» (ядро)
//! и «действие» (трейт): приложения по умолчанию живого сеанса KDE трогать
//! нельзя, а проверить порядок шагов и содержимое записей — обязательно.
//!
//! ⚠️ СТОРОЖ. `guard_*`-сверки следят не за текстами, а за границами:
//! маршрут не смеет писать НИКУДА, кроме подставных XDG-каталогов, и не
//! смеет при входе в систему запускать привилегированные команды. Обе
//! проверены на укус: подмена пути записи на системный и снятие гейта
//! привилегий валят их.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use rexon_core::appdefaults_exec::{
    apply_defaults_route, AppDefaultsEnv, AppDefaultsIo, ComplianceEnv, DeAppSwitcher,
    GenerateOutcome, RunOut,
};
use rexon_core::compliance::{privileged_deferred_note, ComplianceIo, RunResult, ScanEntry};
use rexon_core::system::PkgFamily;

const HOME: &str = "/fake/home";
const CFG: &str = "/fake/cfg";
const DATA: &str = "/fake/data";

// ============================================================================
// Подставной исполнитель
// ============================================================================

/// Записанный побочный вызов: имя операции + аргумент.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Op(String, String);

#[derive(Default)]
struct FakeIo {
    /// Подставная файловая система: путь → содержимое.
    files: RefCell<BTreeMap<String, String>>,
    /// Каталоги (созданные и предзаданные).
    dirs: RefCell<BTreeSet<String>>,
    /// Что «есть в системе» для `which`.
    tools: BTreeSet<String>,
    /// Ответы на запуск команд: первый аргумент argv → (ok, stdout).
    replies: BTreeMap<String, (bool, String)>,
    /// Журнал побочных вызовов в порядке исполнения.
    log: RefCell<Vec<Op>>,
    /// `shutil.which("rexon")`.
    rexon_path: Option<String>,
    /// `/proc/self/exe`.
    self_exe: Option<String>,
    /// Содержимое `/proc/<pid>/comm`.
    comms: BTreeMap<i64, String>,
    /// Ответ пробы окон: `None` — «узнать не удалось» (сеанс Wayland).
    window_pids: Option<Vec<i64>>,
    /// Выбранный вход продукта; `None` — Python-вход (прежние сверки).
    entry: Option<rexon_core::installer::LaunchEntry>,
}

impl FakeIo {
    fn new() -> Self {
        let me = FakeIo {
            self_exe: Some("/opt/rexon/bin/rexon".to_string()),
            ..FakeIo::default()
        };
        me.dirs.borrow_mut().insert(CFG.to_string());
        me.dirs.borrow_mut().insert(DATA.to_string());
        me
    }

    fn file(self, path: &str, content: &str) -> Self {
        self.files.borrow_mut().insert(path.to_string(), content.to_string());
        self
    }

    /// Установленный `.desktop` в системном каталоге.
    fn desktop(self, id: &str) -> Self {
        self.file(&format!("/usr/share/applications/{id}"), "[Desktop Entry]\n")
    }

    fn tool(mut self, name: &str) -> Self {
        self.tools.insert(name.to_string());
        self
    }

    fn reply(mut self, bin: &str, ok: bool, stdout: &str) -> Self {
        self.replies.insert(bin.to_string(), (ok, stdout.to_string()));
        self
    }

    fn ops(&self) -> Vec<Op> {
        self.log.borrow().clone()
    }

    /// Пути, в которые исполнитель ПИСАЛ (запись файла или создание каталога).
    fn written_paths(&self) -> Vec<String> {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "write_text" || o.0 == "makedirs")
            .map(|o| o.1)
            .collect()
    }

    /// argv всех запущенных команд.
    fn commands(&self) -> Vec<String> {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "run")
            .map(|o| o.1)
            .collect()
    }

    fn content(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }

    fn record(&self, op: &str, arg: &str) {
        self.log.borrow_mut().push(Op(op.to_string(), arg.to_string()));
    }
}

impl AppDefaultsIo for FakeIo {
    fn exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn which(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    fn which_path(&self, name: &str) -> Option<String> {
        if name == "rexon" {
            return self.rexon_path.clone();
        }
        self.tools.contains(name).then(|| format!("/usr/bin/{name}"))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        self.record("read_text", path);
        self.files.borrow().get(path).cloned()
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.record("write_text", path);
        self.files.borrow_mut().insert(path.to_string(), content.to_string());
        Ok(())
    }

    fn makedirs(&self, path: &str) -> Result<(), String> {
        self.record("makedirs", path);
        self.dirs.borrow_mut().insert(path.to_string());
        Ok(())
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let mut names: Vec<String> = self
            .files
            .borrow()
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix).map(str::to_string))
            .filter(|n| !n.contains('/'))
            .collect();
        names.sort();
        names
    }

    fn run(&self, argv: &[String]) -> Option<RunOut> {
        self.record("run", &argv.join(" "));
        let bin = argv.first()?.clone();
        self.replies.get(&bin).map(|(ok, stdout)| RunOut {
            ok: *ok,
            stdout: stdout.clone(),
        })
    }

    fn proc_comm(&self, pid: i64) -> Option<String> {
        self.comms.get(&pid).cloned()
    }

    fn window_pids(&self) -> Option<Vec<i64>> {
        self.window_pids.clone()
    }

    fn terminate(&self, pid: i64) {
        self.record("terminate", &pid.to_string());
    }

    fn self_exe(&self) -> Option<String> {
        self.self_exe.clone()
    }

    fn launch_entry(&self) -> rexon_core::installer::LaunchEntry {
        self.entry
            .clone()
            .unwrap_or(rexon_core::installer::LaunchEntry::Python)
    }
}

/// Окружение с подставными XDG-каталогами.
fn env_for(de: &str) -> AppDefaultsEnv {
    AppDefaultsEnv {
        home: HOME.to_string(),
        xdg_config_home: Some(CFG.to_string()),
        xdg_data_home: Some(DATA.to_string()),
        xdg_current_desktop: de.to_string(),
        desktop_session: String::new(),
        has_wayland: false,
    }
}

/// Подставная машина с установленным KDE.
fn kde_io() -> FakeIo {
    FakeIo::new()
        .desktop("org.kde.dolphin.desktop")
        .desktop("org.kde.konsole.desktop")
        .desktop("org.kde.kate.desktop")
        .desktop("org.kde.gwenview.desktop")
        .desktop("org.kde.okular.desktop")
        .desktop("org.kde.ark.desktop")
        .tool("kwriteconfig6")
        .reply("kwriteconfig6", true, "")
}

// ============================================================================
// generate_profile / apply_for_current_de
// ============================================================================

#[test]
fn profile_kde_written_into_fake_config_home() {
    let io = kde_io();
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let out = sw.generate_profile("kde");
    let GenerateOutcome::Wrote(path) = out else {
        panic!("ожидали запись профиля, вышло {out:?}");
    };
    assert_eq!(path, format!("{CFG}/kde-mimeapps.list"));

    let text = io.content(&path).expect("файл профиля");
    assert!(text.starts_with("[Default Applications]\n"), "{text}");
    assert!(text.contains("inode/directory=org.kde.dolphin.desktop"), "{text}");
    // Терминал через MIME не назначается — у него нет типа.
    assert!(!text.contains("konsole"), "{text}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn profile_skipped_when_de_not_installed() {
    let _lang = i18n_pin::ru();
    // Ни одного .desktop: GNOME не установлен.
    let io = FakeIo::new();
    let sw = DeAppSwitcher::new(&io, env_for("GNOME"));
    let out = sw.generate_profile("gnome");
    assert_eq!(out, GenerateOutcome::Skipped("gnome: не установлен, пропуск".into()));
    // Ни одной записи на диск.
    assert!(io.written_paths().is_empty(), "{:?}", io.written_paths());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn apply_for_current_de_kde_full_pass() {
    let _lang = i18n_pin::ru();
    let io = kde_io();
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[0], "🖥️ Текущий рабочий стол: kde");
    assert_eq!(logs[1], "📝 Создан kde-mimeapps.list");
    assert_eq!(logs[2], "✅ KDE терминал: konsole");

    // Порядок действий: сначала профиль, потом настройка терминала, и лишь
    // затем слот «Показать в папке» — так журнал читается сверху вниз.
    let cmds = io.commands();
    assert_eq!(cmds.len(), 4, "{cmds:?}");
    assert!(cmds[0].starts_with("kwriteconfig6 --file kdeglobals"), "{cmds:?}");
    assert!(cmds[0].ends_with("TerminalApplication konsole"), "{cmds:?}");
    assert!(cmds[1].ends_with("TerminalService org.kde.konsole.desktop"), "{cmds:?}");
    assert_eq!(cmds[2], "xdg-mime query default inode/directory", "{cmds:?}");
    assert_eq!(
        cmds[3],
        "busctl --user --no-pager status org.freedesktop.FileManager1",
        "{cmds:?}"
    );
    // Имя никто не держит — гасить некого.
    assert!(logs.last().unwrap().contains("свободно"), "{logs:?}");
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn apply_for_current_de_keeps_existing_profile() {
    let _lang = i18n_pin::ru();
    let io = kde_io().file(&format!("{CFG}/kde-mimeapps.list"), "старое\n");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[1], "✅ kde-mimeapps.list уже существует");
    // Готовый профиль не перезаписывается.
    assert_eq!(io.content(&format!("{CFG}/kde-mimeapps.list")).unwrap(), "старое\n");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn apply_for_current_de_unknown_desktop() {
    let _lang = i18n_pin::ru();
    let io = FakeIo::new();
    let mut env = env_for("");
    env.desktop_session = String::new();
    let sw = DeAppSwitcher::new(&io, env);

    let (ok, logs) = sw.apply_for_current_de();
    assert!(!ok);
    assert_eq!(logs, vec!["⚠️ Не удалось определить текущий рабочий стол".to_string()]);
    assert!(io.ops().is_empty(), "неопознанное окружение ничего не делает");
}

/// ⭐ ПОЧИНЕНО ПРИ ПЕРЕНОСЕ: оригинал печатал «📝 Создан …» и тогда, когда
/// окружение не установлено и файла не появилось.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn skip_is_not_reported_as_created_file() {
    let _lang = i18n_pin::ru();
    // XFCE опознан, но ни одного его .desktop в системе нет.
    let io = FakeIo::new();
    let sw = DeAppSwitcher::new(&io, env_for("XFCE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[1], "ℹ️ xfce: не установлен, пропуск");
    assert!(
        !logs.iter().any(|l| l.contains("Создан")),
        "о создании файла речи быть не может: {logs:?}"
    );
}

// ============================================================================
// XFCE: helpers.rc читается и пишется по одному и тому же пути
// ============================================================================

#[test]
fn xfce_helpers_rc_respects_xdg_config_home() {
    let path = format!("{CFG}/xfce4/helpers.rc");
    let io = FakeIo::new()
        .desktop("thunar.desktop")
        .desktop("xfce4-terminal.desktop")
        .desktop("mousepad.desktop")
        .file(&path, "MailReader=thunderbird\n");
    let sw = DeAppSwitcher::new(&io, env_for("XFCE"));

    let logs = sw.apply_de_specific("xfce");
    assert!(!logs.is_empty(), "{logs:?}");

    let text = io.content(&path).expect("helpers.rc");
    // Прежние строки человека сохранены, наши дописаны.
    assert!(text.contains("MailReader=thunderbird"), "{text}");
    assert!(text.contains("TerminalEmulator="), "{text}");

    // Читали и писали ОДИН и тот же путь под подставным XDG_CONFIG_HOME.
    let reads: Vec<String> = io
        .ops()
        .into_iter()
        .filter(|o| o.0 == "read_text")
        .map(|o| o.1)
        .collect();
    assert!(reads.contains(&path), "{reads:?}");
    assert!(io.written_paths().contains(&path), "{:?}", io.written_paths());
}

// ============================================================================
// Автозапуск
// ============================================================================

#[test]
fn autostart_prefers_rexon_from_path() {
    let mut io = kde_io();
    io.rexon_path = Some("/usr/bin/rexon".to_string());
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, notes) = sw.install_autostart();
    assert!(ok, "{notes:?}");
    let path = format!("{CFG}/autostart/rexon-appdefaults.desktop");
    let text = io.content(&path).expect("autostart");
    assert!(text.contains("Exec=/usr/bin/rexon --apply-defaults"), "{text}");
}

/// ⭐ ПОЧИНЕНО ПРИ ПЕРЕНОСЕ: оригинал без `rexon` в PATH писал в автозапуск
/// путь `/usr/local/lib/rexon/rexon_apply_defaults.sh`, создать который не
/// мог (нет прав), — и автозапуск указывал в пустоту.
#[test]
fn autostart_falls_back_to_own_binary_not_to_system_path() {
    let mut io = kde_io();
    io.rexon_path = None;
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, notes) = sw.install_autostart();
    assert!(ok, "{notes:?}");
    let path = format!("{CFG}/autostart/rexon-appdefaults.desktop");
    let text = io.content(&path).expect("autostart");
    assert!(text.contains("Exec=/opt/rexon/bin/rexon --apply-defaults"), "{text}");
    assert!(!text.contains("/usr/local/lib/rexon"), "{text}");
    assert!(
        !io.written_paths().iter().any(|p| p.starts_with("/usr/")),
        "в системные каталоги маршрут не пишет: {:?}",
        io.written_paths()
    );
}

/// Rust-вход: автозапуск дефолтов зовёт бинарь окна, даже когда лаунчер
/// `rexon` в PATH есть, — интерпретатор при входе в сеанс не поднимается.
#[test]
fn autostart_follows_rust_entry() {
    let path = format!("{CFG}/autostart/rexon-appdefaults.desktop");
    let mut io = kde_io();
    io.rexon_path = Some("/usr/bin/rexon".to_string());
    io.entry = Some(rexon_core::installer::LaunchEntry::MainWindow(
        "/opt/Rexon tree/rexon-main-window".into(),
    ));
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (ok, notes) = sw.install_autostart();
    assert!(ok, "{notes:?}");
    let text = io.content(&path).expect("autostart");
    assert!(
        text.contains("\nExec=\"/opt/Rexon tree/rexon-main-window\" --apply-defaults\n"),
        "{text}"
    );
    assert!(!text.contains("/usr/bin/rexon") && !text.contains("python"), "{text}");

    // ⭐ Откат с пробелом в пути лаунчера: путь в кавычках, а не два аргумента.
    let mut io = kde_io();
    io.rexon_path = Some("/home/u/my bin/rexon".to_string());
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    assert!(sw.install_autostart().0);
    let text = io.content(&path).expect("autostart");
    assert!(text.contains("\nExec=\"/home/u/my bin/rexon\" --apply-defaults\n"), "{text}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn autostart_says_why_it_did_not_install() {
    let _lang = i18n_pin::ru();
    let mut io = kde_io();
    io.rexon_path = None;
    io.self_exe = None;
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, notes) = sw.install_autostart();
    assert!(!ok);
    assert!(notes[0].contains("не нашли, чем запускать"), "{notes:?}");
    assert!(io.written_paths().is_empty(), "{:?}", io.written_paths());
}

// ============================================================================
// FileManager1 + файловый менеджер по умолчанию
// ============================================================================

const NAUTILUS_SERVICE: &str = "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/nautilus --gapplication-service\n";

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn file_manager_default_writes_override_and_evicts_owner() {
    let _lang = i18n_pin::ru();
    let io = kde_io()
        .file(
            "/usr/share/dbus-1/services/org.freedesktop.FileManager1.service",
            NAUTILUS_SERVICE,
        )
        .file(
            "/usr/share/dbus-1/services/org.kde.dolphin.service",
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/dolphin --daemon\n",
        )
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, NAUTILUS_OWNER)
        .reply("/usr/bin/nautilus", true, "");

    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (applied, notes) = sw.set_file_manager_default("dolphin", Some("kde"));
    assert!(applied, "{notes:?}");

    // Override лёг в ПОДСТАВНОЙ XDG_DATA_HOME.
    let target = format!("{DATA}/dbus-1/services/org.freedesktop.FileManager1.service");
    let text = io.content(&target).expect("override");
    assert!(text.contains("Exec=/usr/bin/dolphin"), "{text}");

    // Чужой демон погашен ШТАТНО — своим бинарём, не сигналом и не по имени.
    assert!(
        io.commands().iter().any(|c| c == "/usr/bin/nautilus --quit"),
        "{:?}",
        io.commands()
    );
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
    assert!(notes.iter().any(|n| n.contains("завершён штатно")), "{notes:?}");
}

/// Живой вывод `busctl status` для чужого демона (nautilus в режиме службы).
const NAUTILUS_OWNER: &str = "PID=4242\nComm=nautilus\nExe=/usr/bin/nautilus\n\
CommandLine=/usr/bin/nautilus --gapplication-service\n";

/// Живой вывод `busctl status` для демона Thunar, поднятого KDE из
/// сохранённой сессии, — тот самый случай, ради которого маршрут и учили.
const THUNAR_OWNER: &str = "PID=2583\nComm=Thunar\nExe=/usr/bin/Thunar\n\
CommandLine=/usr/bin/Thunar --sm-client-id 10188108 --daemon\n";

const KSM_WITH_THUNAR: &str = "[General]\nexcludeApps=\n\n\
[Session: saved at previous logout]\ncount=1\nprogram1=Thunar\n\
restartCommand1=Thunar,--sm-client-id,10188108,--daemon\n";

/// Машина владельца 09.09.2026: mime-умолчания ВЕРНЫЕ (Dolphin), а
/// «Открыть содержащую папку» открывает Thunar, потому что имя на шине
/// держит его демон из сохранённой сессии KDE.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn login_route_takes_filemanager1_back_from_foreign_daemon() {
    let _lang = i18n_pin::ru();
    let io = kde_io()
        .file(
            "/usr/share/dbus-1/services/org.kde.dolphin.FileManager1.service",
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/dolphin --daemon\n",
        )
        .file(&format!("{CFG}/ksmserverrc"), KSM_WITH_THUNAR)
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, THUNAR_OWNER)
        .reply("/usr/bin/Thunar", true, "");

    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok, "{logs:?}");

    let all = logs.join("\n");
    assert!(all.contains("перехватывал thunar (pid 2583"), "{all}");

    // Погашен штатно и ИМЕННО тот бинарь, что назвала шина.
    assert!(
        io.commands().iter().any(|c| c == "/usr/bin/Thunar --quit"),
        "{:?}",
        io.commands()
    );
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());

    // Из сохранённой сессии KDE запись убрана, имя внесено в исключения —
    // иначе следующий вход поднял бы демон снова.
    let ksm = io.content(&format!("{CFG}/ksmserverrc")).expect("ksmserverrc");
    assert!(!ksm.contains("Thunar"), "{ksm}");
    assert!(ksm.contains("excludeApps=thunar"), "{ksm}");
    assert!(all.contains("убран из сохранённой сессии KDE"), "{all}");

    // Наш override активации лёг рядом и указывает на Dolphin.
    let target = format!("{DATA}/dbus-1/services/org.freedesktop.FileManager1.service");
    assert!(io.content(&target).unwrap().contains("dolphin"), "нет override");
}

/// ⛔ У чужого менеджера открыты окна — маршрут его НЕ трогает.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn login_route_never_kills_file_manager_with_open_windows() {
    let _lang = i18n_pin::ru();
    let mut io = kde_io()
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, THUNAR_OWNER)
        .reply("/usr/bin/Thunar", true, "");
    // Проба окон сказала: у этого PID окно есть.
    io.window_pids = Some(vec![2583]);

    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (_, logs) = sw.apply_for_current_de();
    let all = logs.join("\n");
    assert!(all.contains("открыты окна"), "{all}");
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
    assert!(
        !io.commands().iter().any(|c| c.contains("--quit")),
        "{:?}",
        io.commands()
    );
}

/// Имя держит тот, кто и назначен умолчанием, — ничего не делаем.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn login_route_leaves_matching_owner_alone() {
    let _lang = i18n_pin::ru();
    let io = kde_io()
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply(
            "busctl",
            true,
            "PID=77\nComm=dolphin\nExe=/usr/bin/dolphin\nCommandLine=/usr/bin/dolphin\n",
        );
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (_, logs) = sw.apply_for_current_de();
    assert!(
        logs.iter().any(|l| l.contains("имя держит dolphin")),
        "{logs:?}"
    );
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
}

/// Менеджер без своей команды выхода гасится сигналом РОВНО тому PID,
/// который назвала шина.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn foreign_daemon_without_quit_option_gets_signal_by_pid() {
    let _lang = i18n_pin::ru();
    let io = kde_io().reply("xdg-mime", true, "nemo.desktop\n").reply(
        "busctl",
        true,
        "PID=31\nComm=dolphin\nExe=/usr/bin/dolphin\nCommandLine=/usr/bin/dolphin --daemon\n",
    );
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let mut notes = Vec::new();
    assert!(sw.reclaim_filemanager1("nemo", &mut notes));
    assert!(io.ops().contains(&Op("terminate".into(), "31".into())), "{:?}", io.ops());
    assert!(notes.iter().any(|n| n.contains("сигнал завершения")), "{notes:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn file_manager_default_without_vendor_service_says_so() {
    let _lang = i18n_pin::ru();
    let io = kde_io()
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, "");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (applied, notes) = sw.set_file_manager_default("dolphin", Some("kde"));
    assert!(applied, "{notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("нет собственного файла службы D-Bus")),
        "{notes:?}"
    );
    assert!(io.written_paths().is_empty(), "{:?}", io.written_paths());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn file_manager_default_missing_desktop_is_reported() {
    let _lang = i18n_pin::ru();
    let io = FakeIo::new();
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (applied, notes) = sw.set_file_manager_default("nemo", None);
    assert!(!applied);
    assert!(notes[0].contains("не установлен"), "{notes:?}");
    assert!(io.commands().is_empty(), "команд не запускаем: {:?}", io.commands());
}

// ============================================================================
// Маршрут целиком
// ============================================================================

/// Подставной исполнитель проверки комплектности: безопасные умолчания
/// (`safe_run` → None, файлов нет) + журнал команд.
#[derive(Default)]
struct FakeComplianceIo {
    commands: Vec<Vec<String>>,
}

impl ComplianceIo for FakeComplianceIo {
    fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<RunResult> {
        self.commands.push(argv.to_vec());
        None
    }
    fn which(&mut self, _name: &str) -> bool {
        false
    }
    fn env(&mut self, _name: &str) -> String {
        String::new()
    }
    fn exists(&mut self, _path: &str) -> bool {
        false
    }
    fn is_file(&mut self, _path: &str) -> bool {
        false
    }
    fn is_dir(&mut self, _path: &str) -> bool {
        false
    }
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
    fn scandir(&mut self, _path: &str) -> Option<Vec<ScanEntry>> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn makedirs(&mut self, path: &str) {
        panic!("проверка при входе в систему не смеет создавать {path}");
    }
    fn write_text(&mut self, path: &str, _content: &str) {
        panic!("проверка при входе в систему не смеет писать {path}");
    }
    fn replace(&mut self, _src: &str, dst: &str) {
        panic!("проверка при входе в систему не смеет заменять {dst}");
    }
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn mkdtemp(&mut self, prefix: &str, _dir: Option<&str>) -> String {
        panic!("проверка при входе в систему не смеет заводить {prefix}");
    }
    fn mkstemp(&mut self, prefix: &str, _suffix: &str) -> String {
        panic!("проверка при входе в систему не смеет заводить {prefix}");
    }
    fn chmod(&mut self, _path: &str, _mode: u32) {}
    fn remove_file(&mut self, _path: &str) {}
    fn remove_tree(&mut self, _path: &str) {}
    fn copy_file(&mut self, _src: &str, dst: &str) {
        panic!("проверка при входе в систему не смеет копировать в {dst}");
    }
}

fn arch_env() -> ComplianceEnv {
    ComplianceEnv {
        pkg_family: PkgFamily::Arch,
        pkg_helper_available: false,
        stage_helper_available: false,
    }
}

/// ⭐ Помощники polkit лежат в ДВУХ раскладках, и диагностика обязана видеть
/// обе: пакет дистрибутива кладёт их в `/usr/lib`, ручная установка — в
/// `/usr/local/lib`. Прибитая константа `/usr/local/lib` давала на машине с
/// пакетом «помощников нет» там, где они есть.
#[test]
fn diagnosis_finds_helpers_in_both_layouts() {
    const PKG: &str = "/usr/lib/rexon-pkg-helper";
    const STAGE: &str = "/usr/lib/rexon-stage-helper";
    const PKG_LOCAL: &str = "/usr/local/lib/rexon-pkg-helper";
    const STAGE_LOCAL: &str = "/usr/local/lib/rexon-stage-helper";
    const PKG_POLICY: &str = "/usr/share/polkit-1/actions/org.rexon.pkg.policy";
    const STAGE_POLICY: &str = "/usr/share/polkit-1/actions/org.rexon.stage.policy";

    // Раскладка на подставных пробах: что «есть», то и видно.
    let layout = |files: &'static [&'static str]| {
        move |p: &str| files.contains(&p) || p == PKG_POLICY || p == STAGE_POLICY
    };
    let probe = |files: &'static [&'static str]| {
        let seen = layout(files);
        ComplianceEnv::probe(PkgFamily::Arch, true, &seen, &seen)
    };

    // Машина с пакетом: помощники в /usr/lib.
    let pkg_machine = probe(&[PKG, STAGE]);
    assert!(pkg_machine.pkg_helper_available, "пакет: помощник в /usr/lib не найден");
    assert!(pkg_machine.stage_helper_available, "пакет: помощник в /usr/lib не найден");

    // Машина с ручной установкой: помощники в /usr/local/lib.
    let manual = probe(&[PKG_LOCAL, STAGE_LOCAL]);
    assert!(manual.pkg_helper_available, "ручная установка: помощник не найден");
    assert!(manual.stage_helper_available, "ручная установка: помощник не найден");

    // Обе раскладки сразу — по-прежнему «есть»; ни одной — честное «нет».
    let both = probe(&[PKG, STAGE, PKG_LOCAL, STAGE_LOCAL]);
    assert_eq!(both, pkg_machine);
    let bare = probe(&[]);
    assert!(!bare.pkg_helper_available && !bare.stage_helper_available, "{bare:?}");

    // Помощники на месте, а pkexec в системе нет — спросить пароль нечем.
    let seen = layout(&[PKG, STAGE]);
    let no_pkexec = ComplianceEnv::probe(PkgFamily::Arch, false, &seen, &seen);
    assert!(!no_pkexec.pkg_helper_available && !no_pkexec.stage_helper_available);

    // Помощник есть, а его политики нет — привилегированного пути нет.
    let no_policy = |p: &str| p == PKG || p == STAGE;
    let unpolicied = ComplianceEnv::probe(PkgFamily::Arch, true, &no_policy, &no_policy);
    assert!(!unpolicied.pkg_helper_available && !unpolicied.stage_helper_available);
}

fn run_route(io: &FakeIo, de: &str) -> (i32, Vec<String>, FakeComplianceIo) {
    let mut cio = FakeComplianceIo::default();
    let mut printed: Vec<String> = Vec::new();
    let code = apply_defaults_route(io, &env_for(de), &mut cio, &arch_env(), &mut |l| {
        printed.push(l.to_string())
    });
    (code, printed, cio)
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn route_applies_then_checks() {
    let _lang = i18n_pin::ru();
    let io = kde_io();
    let (code, printed, _) = run_route(&io, "KDE");

    assert_eq!(code, 0);
    // Сначала — весь вывод переключателя, потом — проверка комплектности.
    assert_eq!(printed[0], "🖥️ Текущий рабочий стол: kde");
    let check_at = printed
        .iter()
        .position(|l| l.starts_with("🔍 Проверка рабочего стола"))
        .expect("проверка комплектности в выводе");
    assert!(check_at >= 3, "проверка идёт ПОСЛЕ применения: {printed:?}");
    assert!(printed.last().unwrap().starts_with("✅ Проверка завершена"), "{printed:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn route_returns_one_when_desktop_unknown() {
    let _lang = i18n_pin::ru();
    let io = FakeIo::new();
    let (code, printed, _) = run_route(&io, "");
    assert_eq!(code, 1);
    assert_eq!(printed[0], "⚠️ Не удалось определить текущий рабочий стол");
}

// ============================================================================
// СТОРОЖ
// ============================================================================

/// Сторож: маршрут пишет ТОЛЬКО в подставные XDG-каталоги.
///
/// Живая машина при сверке не меняется — это условие задачи, а не пожелание:
/// приложения по умолчанию живого сеанса и системные каталоги трогать
/// нельзя. Укус проверен: стоит подставить исполнителю системный путь
/// (например, вернуть автозапуску `/usr/local/lib/rexon/...`), сверка падает.
#[test]
fn guard_route_writes_only_under_fake_xdg_dirs() {
    let mut io = kde_io();
    io.rexon_path = None; // самая опасная ветка: запасной путь автозапуска
    let (_, _, _) = run_route(&io, "KDE");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    sw.install_autostart();
    sw.set_file_manager_default("dolphin", Some("kde"));

    let written = io.written_paths();
    assert!(!written.is_empty(), "сверка должна что-то писать, иначе она пустая");
    for path in &written {
        let allowed = path.starts_with(&format!("{CFG}/")) || path.starts_with(&format!("{DATA}/"));
        assert!(
            allowed,
            "маршрут пишет мимо подставных XDG-каталогов: {path} (весь список: {written:?})"
        );
        assert!(
            !path.starts_with(HOME) && !path.starts_with("/usr") && !path.starts_with("/etc"),
            "запись в живой каталог: {path}"
        );
    }
}

/// Сторож: при входе в систему маршрут не запускает НИЧЕГО привилегированного
/// и не ходит в сеть.
///
/// Окружение взято MATE НАМЕРЕННО: именно у него лечение привилегированное —
/// доустановка пакетов ayatana под `pkexec`, AUR-хелпер, `git clone`
/// раскладок панели и копирование их в системный каталог. На подставной
/// машине этих пакетов нет, то есть ветка лечения достижима, и весь вопрос в
/// гейте `allow_privileged = false`. Подставной исполнитель проверки вдобавок
/// роняет сверку на любой попытке записи или временного каталога. Укус
/// проверен: замена гейта на `true` в [`apply_defaults_route`] валит сверку
/// на `pkexec pacman` в списке команд.
#[test]
fn guard_login_route_stays_unprivileged() {
    let io = FakeIo::new().desktop("caja.desktop").desktop("mate-terminal.desktop");
    let (_, printed, cio) = run_route(&io, "MATE");
    assert!(
        printed.iter().any(|l| *l == privileged_deferred_note()),
        "лечение обязано быть ОТЛОЖЕНО, иначе сверка ничего не проверяет: {printed:?}"
    );

    for argv in &cio.commands {
        let head = argv.first().map(String::as_str).unwrap_or("");
        assert!(
            !matches!(head, "pkexec" | "sudo" | "doas" | "git" | "curl" | "wget"),
            "при входе в систему такой команды быть не должно: {argv:?}"
        );
    }
    for argv in &io.commands() {
        assert!(
            !argv.starts_with("pkexec") && !argv.starts_with("sudo"),
            "переключатель дефолтов не привилегированный: {argv}"
        );
    }
    // Строка об отложенном лечении — не больше одной на прогон.
    let deferred = printed.iter().filter(|l| **l == privileged_deferred_note()).count();
    assert!(deferred <= 1, "строка об отложенном лечении не должна повторяться");
}

/// ЖИВАЯ проверка НАБЛЮДЕНИЯ (ничего не меняет и никого не гасит): спросить
/// у настоящей шины сеанса, кто держит `org.freedesktop.FileManager1`, и
/// убедиться, что разбор его понимает.
///
/// Зачем отдельно от подставных сверок: `busctl status` печатает `Comm`,
/// `Exe` и `CommandLine` не на всех версиях systemd, и если поле пропадёт,
/// решение молча съедет в осторожную ветку «не трогаем» — дефект вернётся, а
/// сверки останутся зелёными. Здесь только чтение: ни одной команды, кроме
/// `busctl status`, тест не запускает.
#[test]
#[cfg_attr(
    feature = "isolated-checks",
    ignore = "нужна живая машина: спрашивает настоящую шину сеанса"
)]
fn live_bus_owner_of_filemanager1_is_understood() {
    use rexon_core::appdefaults as ad;
    use rexon_core::appdefaults_exec::LiveAppDefaultsIo;

    let io = LiveAppDefaultsIo;
    let Some(res) = io.run(&ad::busctl_status_cmd(ad::FM1_NAME)) else {
        return; // нет шины сеанса (CI) — проверять нечего
    };
    if !res.ok || !res.stdout.contains("PID=") {
        return; // имя никто не держит — тоже законный исход
    }
    let facts = ad::busctl_owner(&res.stdout).expect("вывод шины обязан разбираться");
    assert!(facts.pid > 0, "{}", res.stdout);
    assert!(
        !facts.exe.is_empty() || !facts.comm.is_empty(),
        "шина перестала называть владельца — решение о слоте станет слепым: {}",
        res.stdout
    );
    // Живая машина владельца: имя держит файловый менеджер, и он опознаётся.
    if let Some(short) = ad::fm_short_from_process(&facts.comm, &facts.exe) {
        assert!(ad::fm_daemon_info(&short).is_some(), "нет записи в FM_DAEMONS: {short}");
    }
}

// ============================================================================
// Английский интерфейс
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Журнал переключателя по-английски — без русских букв, фразами.
#[test]
fn english_switcher_log_has_no_russian() {
    let _lang = i18n_pin::en();
    let io = kde_io();
    let (ok, logs) = DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[0], "🖥️ Current desktop: kde");
    assert_eq!(logs[2], "✅ KDE terminal: konsole");

    let io = kde_io()
        .file(
            "/usr/share/dbus-1/services/org.kde.dolphin.FileManager1.service",
            "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/dolphin --daemon\n",
        )
        .file(&format!("{CFG}/ksmserverrc"), KSM_WITH_THUNAR)
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, THUNAR_OWNER)
        .reply("/usr/bin/Thunar", true, "");
    let (_, thunar) = DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert!(
        thunar.iter().any(|l| l.contains("took over “Show in Folder” instead of")),
        "{thunar:?}"
    );

    let io = FakeIo::new();
    let (ok, unknown) = DeAppSwitcher::new(&io, env_for("")).apply_for_current_de();
    assert!(!ok);
    let gnome = DeAppSwitcher::new(&io, env_for("GNOME")).generate_profile("gnome");
    assert_eq!(gnome, GenerateOutcome::Skipped("gnome: not installed, skipping".into()));

    for line in logs.iter().chain(&thunar).chain(&unknown) {
        assert!(!has_cyrillic(line), "{line}");
    }
}
