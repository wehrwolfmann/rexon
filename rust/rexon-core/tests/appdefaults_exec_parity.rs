//! Checks of the live executor of the `--apply-defaults` route
//! ([`rexon_core::appdefaults_exec`]).
//!
//! HOW THIS IS CHECKED. The executor is run on FAKE I/O
//! ([`FakeIo`]) with fake `$HOME`/`$XDG_CONFIG_HOME`/`$XDG_DATA_HOME`:
//! not a single file of the live machine is read or written, not a single subprocess is
//! launched. This is exactly why the executor is split into "decision" (the core)
//! and "action" (a trait): the default applications of the live KDE session must not
//! be touched, while checking the order of steps and the content of the writes is mandatory.
//!
//! ⚠️ GUARD. The `guard_*` checks watch not texts but boundaries:
//! the route must not write ANYWHERE except the fake XDG directories, and must
//! not run privileged commands at login. Both
//! were bite-tested: replacing the write path with a system one and removing the privilege
//! gate make them fail.

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
// Fake executor
// ============================================================================

/// A recorded side-effect call: operation name + argument.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Op(String, String);

#[derive(Default)]
struct FakeIo {
    /// Fake file system: path → contents.
    files: RefCell<BTreeMap<String, String>>,
    /// Directories (created and preset).
    dirs: RefCell<BTreeSet<String>>,
    /// What is "on the system" for `which`.
    tools: BTreeSet<String>,
    /// Answers to command launches: the first argv argument → (ok, stdout).
    replies: BTreeMap<String, (bool, String)>,
    /// Log of side-effect calls in execution order.
    log: RefCell<Vec<Op>>,
    /// `shutil.which("rexon")`.
    rexon_path: Option<String>,
    /// `/proc/self/exe`.
    self_exe: Option<String>,
    /// Contents of `/proc/<pid>/comm`.
    comms: BTreeMap<i64, String>,
    /// The window probe's answer: `None` — "could not find out" (a Wayland session).
    window_pids: Option<Vec<i64>>,
    /// The chosen product entry; `None` — the Python entry (the old checks).
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

    /// An installed `.desktop` in the system directory.
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

    /// Paths the executor WROTE to (a file write or a directory creation).
    fn written_paths(&self) -> Vec<String> {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "write_text" || o.0 == "makedirs")
            .map(|o| o.1)
            .collect()
    }

    /// argv of all launched commands.
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

    fn remove_file(&self, path: &str) -> Result<(), String> {
        self.record("remove_file", path);
        self.files.borrow_mut().remove(path);
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

/// Environment with fake XDG directories.
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

/// A fake machine with KDE installed.
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
    assert!(text.starts_with("# Rexon: per-desktop defaults\n\n[Default Applications]\n"), "{text}");
    assert!(text.contains("inode/directory=org.kde.dolphin.desktop"), "{text}");
    // The terminal is not assigned through MIME — it has no type.
    assert!(!text.contains("konsole"), "{text}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn profile_skipped_when_de_not_installed() {
    let _lang = i18n_pin::ru();
    // Not a single .desktop: GNOME is not installed.
    let io = FakeIo::new();
    let sw = DeAppSwitcher::new(&io, env_for("GNOME"));
    let out = sw.generate_profile("gnome");
    assert_eq!(out, GenerateOutcome::Skipped("gnome: не установлен, пропуск".into()));
    // Not a single write to disk.
    assert!(io.written_paths().is_empty(), "{:?}", io.written_paths());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn apply_for_current_de_kde_full_pass() {
    let _lang = i18n_pin::ru();
    let io = kde_io();
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[0], "🖥️ Текущий рабочий стол: kde");
    assert_eq!(logs[1], "📝 Создан kde-mimeapps.list");
    assert_eq!(logs[2], "📝 Создан kde-xdg-terminals.list");
    assert_eq!(logs[3], "✅ KDE терминал: konsole");

    // Order of actions: first the profile, then the terminal setup, and only
    // then the «Показать в папке» slot — so the log reads top to bottom.
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
    // Nobody holds the name — nobody to stop.
    assert!(logs.last().unwrap().contains("свободно"), "{logs:?}");
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn apply_for_current_de_keeps_existing_profile() {
    let _lang = i18n_pin::ru();
    let io = kde_io().file(&format!("{CFG}/kde-mimeapps.list"), "старое\n");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[1], "✅ kde-mimeapps.list уже существует");
    // A ready profile is not overwritten.
    assert_eq!(io.content(&format!("{CFG}/kde-mimeapps.list")).unwrap(), "старое\n");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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

/// ⭐ FIXED DURING THE MOVE: the original printed «📝 Создан …» even when
/// the desktop was not installed and no file appeared.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn skip_is_not_reported_as_created_file() {
    let _lang = i18n_pin::ru();
    // XFCE is recognized, but not a single one of its .desktop files is on the system.
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
// The desktop's own terminal list (`<desktop>-xdg-terminals.list`)
// ============================================================================

/// A fake machine with MATE where foreign terminals are installed too.
fn mate_io() -> FakeIo {
    FakeIo::new()
        .desktop("caja.desktop")
        .desktop("mate-terminal.desktop")
        .desktop("org.gnome.Terminal.desktop")
        .desktop("org.kde.konsole.desktop")
}

/// The 08.11.2026 case from the owner's live machine: in MATE a foreign
/// `mate-xdg-terminals.list` named GNOME Terminal, the common list named konsole,
/// and a program with `Terminal=true` opened not in mate-terminal. Logging into the session
/// fixes the desktop's own list; the common one is not touched.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn login_fixes_foreign_desktop_terminals_list() {
    let _lang = i18n_pin::ru();
    let list = format!("{CFG}/mate-xdg-terminals.list");
    let general = format!("{CFG}/xdg-terminals.list");
    let io = mate_io()
        .file(&list, "org.gnome.Terminal.desktop\n")
        .file(&general, "org.kde.konsole.desktop\n");
    let sw = DeAppSwitcher::new(&io, env_for("MATE"));

    let (ok, logs) = sw.apply_for_current_de();
    assert!(ok);
    let text = io.content(&list).unwrap();
    assert_eq!(text.lines().next(), Some("mate-terminal.desktop"), "{text}");
    assert!(text.contains(rexon_core::defaults::TERMINALS_LIST_NATIVE_MARK), "{text}");
    assert!(logs.contains(&"✅ Исправлено: терминал → mate-terminal".to_string()), "{logs:?}");
    assert_eq!(io.content(&general).unwrap(), "org.kde.konsole.desktop\n");

    // A second login rewrites nothing.
    let before = io.written_paths().len();
    let (_, logs) = sw.apply_for_current_de();
    assert!(!io.written_paths()[before..].contains(&list), "{logs:?}");
    assert!(logs.contains(&"✅ mate-xdg-terminals.list уже существует".to_string()), "{logs:?}");
}

/// A deliberate choice in the «Настройки» tab (marked "chosen by the person")
/// is not replaced by the native terminal at login.
#[test]
fn login_keeps_terminal_chosen_in_rexon() {
    let list = format!("{CFG}/mate-xdg-terminals.list");
    let chosen = rexon_core::defaults::desktop_terminals_list_text("org.kde.konsole.desktop", true);
    let io = mate_io().file(&list, &chosen);
    let sw = DeAppSwitcher::new(&io, env_for("MATE"));

    sw.apply_for_current_de();
    assert_eq!(io.content(&list).unwrap(), chosen);
    assert!(!io.written_paths().contains(&list), "{:?}", io.written_paths());

    // And it is the one that goes into the desktop's native place (the MATE schema), not mate-terminal.
    let io = mate_io().tool("gsettings").file(&list, &chosen);
    DeAppSwitcher::new(&io, env_for("MATE")).apply_for_current_de();
    let cmds = io.commands();
    assert!(cmds.contains(&"gsettings set org.mate.applications-terminal exec konsole".to_string()), "{cmds:?}");
    assert!(!cmds.iter().any(|c| c.contains("exec mate-terminal")), "{cmds:?}");
}

/// Switching the desktop through Rexon creates the target desktop's list by its
/// identifier (Cinnamon — `x-cinnamon-…`, like `x-cinnamon-mimeapps.list`).
#[test]
fn switch_de_writes_target_desktop_terminals_list() {
    let io = FakeIo::new()
        .desktop("nemo.desktop")
        .desktop("org.gnome.Terminal.desktop");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let mut logs = Vec::new();
    sw.on_switch_de("cinnamon-mint", &mut logs);
    let text = io.content(&format!("{CFG}/x-cinnamon-xdg-terminals.list")).expect("список Cinnamon");
    assert_eq!(text.lines().next(), Some("org.gnome.Terminal.desktop"), "{text}");
}

// ============================================================================
// XFCE: helpers.rc is read and written at the same path
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
    // The person's former lines are kept, ours are appended.
    assert!(text.contains("MailReader=thunderbird"), "{text}");
    assert!(text.contains("TerminalEmulator="), "{text}");

    // Read and wrote ONE and the same path under the fake XDG_CONFIG_HOME.
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
// Autostart
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

/// ⛔ gnome-session 49+ does not execute third-party entries with
/// `X-GNOME-Autostart-Phase`: in the new entry no entry point has a phase.
#[test]
fn autostart_has_no_gnome_session_phase() {
    let path = format!("{CFG}/autostart/rexon-appdefaults.desktop");
    for rexon in [Some("/usr/bin/rexon"), None] {
        let mut io = kde_io();
        io.rexon_path = rexon.map(str::to_string);
        let sw = DeAppSwitcher::new(&io, env_for("GNOME"));
        assert!(sw.install_autostart().0);
        let text = io.content(&path).expect("autostart");
        assert!(!text.contains("Autostart-Phase"), "{text}");
        assert!(text.contains(" --apply-defaults\n"), "{text}");
    }
}

/// An entry of earlier versions (with a phase) is fixed when logging into a session of another
/// desktop: the phase line is removed, everything else, including the `NotShowIn=` of the compositor
/// mirrors, is in place. The fix does NOT create a missing entry.
#[test]
fn login_heals_legacy_autostart_phase() {
    let path = format!("{CFG}/autostart/rexon-appdefaults.desktop");
    let legacy = "[Desktop Entry]\nType=Application\nName=Rexon App Defaults\n\
                  Exec=/usr/bin/rexon --apply-defaults\nTerminal=false\nNoDisplay=true\n\
                  X-GNOME-Autostart-Phase=Initialization\nNotShowIn=Hyprland;\n";
    let io = kde_io().file(&path, legacy);
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    assert!(sw.apply_for_current_de().0);
    assert_eq!(
        io.content(&path).unwrap(),
        "[Desktop Entry]\nType=Application\nName=Rexon App Defaults\n\
         Exec=/usr/bin/rexon --apply-defaults\nTerminal=false\nNoDisplay=true\n\
         NotShowIn=Hyprland;\n"
    );
    // Nothing to fix a second time — there is no entry.
    let writes_before = io.written_paths().len();
    assert!(!sw.heal_autostart());
    assert_eq!(io.written_paths().len(), writes_before);

    // No entry — and none appears.
    let io = kde_io();
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    assert!(sw.apply_for_current_de().0);
    assert_eq!(io.content(&path), None);
    assert!(!io.written_paths().contains(&path), "{:?}", io.written_paths());
}

#[test]
fn heal_autostart_text_cases() {
    use rexon_core::appdefaults::heal_autostart_text as heal;
    assert_eq!(heal("[Desktop Entry]\nExec=x\n"), None);
    assert_eq!(
        heal("[Desktop Entry]\nX-GNOME-Autostart-Phase = Initialization\nExec=x\n").as_deref(),
        Some("[Desktop Entry]\nExec=x\n")
    );
    // Foreign keys with a similar beginning are not touched.
    assert_eq!(heal("[Desktop Entry]\nX-GNOME-Autostart-Phases-Note=1\n"), None);
    assert_eq!(heal("[Desktop Entry]\nX-GNOME-Autostart-enabled=true\n"), None);
    // The last line without a line break.
    assert_eq!(
        heal("[Desktop Entry]\nExec=x\nX-GNOME-Autostart-Phase=Initialization").as_deref(),
        Some("[Desktop Entry]\nExec=x\n")
    );
}

/// ⭐ FIXED DURING THE MOVE: without `rexon` in PATH the original wrote to autostart
/// the path `/usr/local/lib/rexon/rexon_apply_defaults.sh`, which it could not
/// create (no permissions) — and the autostart pointed into nowhere.
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

/// The Rust entry: the defaults autostart calls the window binary even when the `rexon`
/// launcher is in PATH — no interpreter is started at session login.
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

    // ⭐ Fallback with a space in the launcher path: the path in quotes, not two arguments.
    let mut io = kde_io();
    io.rexon_path = Some("/home/u/my bin/rexon".to_string());
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    assert!(sw.install_autostart().0);
    let text = io.content(&path).expect("autostart");
    assert!(text.contains("\nExec=\"/home/u/my bin/rexon\" --apply-defaults\n"), "{text}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
// FileManager1 + the default file manager
// ============================================================================

const NAUTILUS_SERVICE: &str = "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/nautilus --gapplication-service\n";

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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

    // The override landed in the FAKE XDG_DATA_HOME.
    let target = format!("{DATA}/dbus-1/services/org.freedesktop.FileManager1.service");
    let text = io.content(&target).expect("override");
    assert!(text.contains("Exec=/usr/bin/dolphin"), "{text}");

    // The foreign daemon was stopped THE PROPER WAY — by its own binary, not by a signal or by name.
    assert!(
        io.commands().iter().any(|c| c == "/usr/bin/nautilus --quit"),
        "{:?}",
        io.commands()
    );
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
    assert!(notes.iter().any(|n| n.contains("завершён штатно")), "{notes:?}");
}

/// Live `busctl status` output for a foreign daemon (nautilus in service mode).
const NAUTILUS_OWNER: &str = "PID=4242\nComm=nautilus\nExe=/usr/bin/nautilus\n\
CommandLine=/usr/bin/nautilus --gapplication-service\n";

/// Live `busctl status` output for a Thunar daemon raised by KDE from a
/// saved session — the very case the route was taught for.
const THUNAR_OWNER: &str = "PID=2583\nComm=Thunar\nExe=/usr/bin/Thunar\n\
CommandLine=/usr/bin/Thunar --sm-client-id 10188108 --daemon\n";

const KSM_WITH_THUNAR: &str = "[General]\nexcludeApps=\n\n\
[Session: saved at previous logout]\ncount=1\nprogram1=Thunar\n\
restartCommand1=Thunar,--sm-client-id,10188108,--daemon\n";

/// The owner's machine on 09.09.2026: the mime defaults are CORRECT (Dolphin), but
/// «Открыть содержащую папку» opens Thunar, because the name on the bus
/// is held by its daemon from the saved KDE session.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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

    // Stopped the proper way, and EXACTLY the binary the bus named.
    assert!(
        io.commands().iter().any(|c| c == "/usr/bin/Thunar --quit"),
        "{:?}",
        io.commands()
    );
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());

    // The entry was removed from the saved KDE session, the name added to the exclusions —
    // otherwise the next login would raise the daemon again.
    let ksm = io.content(&format!("{CFG}/ksmserverrc")).expect("ksmserverrc");
    assert!(!ksm.contains("Thunar"), "{ksm}");
    assert!(ksm.contains("excludeApps=thunar"), "{ksm}");
    assert!(all.contains("убран из сохранённой сессии KDE"), "{all}");

    // Our activation override landed next to it and points to Dolphin.
    let target = format!("{DATA}/dbus-1/services/org.freedesktop.FileManager1.service");
    assert!(io.content(&target).unwrap().contains("dolphin"), "нет override");
}

/// ⛔ The foreign manager has open windows — the route does NOT touch it.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn login_route_never_kills_file_manager_with_open_windows() {
    let _lang = i18n_pin::ru();
    let mut io = kde_io()
        .reply("xdg-mime", true, "org.kde.dolphin.desktop\n")
        .reply("busctl", true, THUNAR_OWNER)
        .reply("/usr/bin/Thunar", true, "");
    // The window probe said: this PID has a window.
    io.window_pids = Some(vec![2583]);

    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    let (_, logs) = sw.apply_for_current_de();
    let all = logs.join("\n");
    assert!(all.contains("Закройте его окна"), "{all}");
    assert!(io.ops().iter().all(|o| o.0 != "terminate"), "{:?}", io.ops());
    assert!(
        !io.commands().iter().any(|c| c.contains("--quit")),
        "{:?}",
        io.commands()
    );
}

/// The name is held by the one assigned as default — we do nothing.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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

/// A manager without its own quit command is stopped by a signal to EXACTLY the PID
/// the bus named.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
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
// The whole route
// ============================================================================

/// Fake executor of the completeness check: safe defaults
/// (`safe_run` → None, no files) + a command log.
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

/// ⭐ polkit helpers live in TWO layouts, and diagnostics must see
/// both: a distro package puts them into `/usr/lib`, a manual install into
/// `/usr/local/lib`. A hard-coded constant `/usr/local/lib` gave "no helpers"
/// on a machine with the package where they do exist.
#[test]
fn diagnosis_finds_helpers_in_both_layouts() {
    const PKG: &str = "/usr/lib/rexon-pkg-helper";
    const STAGE: &str = "/usr/lib/rexon-stage-helper";
    const PKG_LOCAL: &str = "/usr/local/lib/rexon-pkg-helper";
    const STAGE_LOCAL: &str = "/usr/local/lib/rexon-stage-helper";
    const PKG_POLICY: &str = "/usr/share/polkit-1/actions/org.rexon.pkg.policy";
    const STAGE_POLICY: &str = "/usr/share/polkit-1/actions/org.rexon.stage.policy";

    // Layout on fake probes: what "exists" is what is seen.
    let layout = |files: &'static [&'static str]| {
        move |p: &str| files.contains(&p) || p == PKG_POLICY || p == STAGE_POLICY
    };
    let probe = |files: &'static [&'static str]| {
        let seen = layout(files);
        ComplianceEnv::probe(PkgFamily::Arch, true, &seen, &seen)
    };

    // A machine with the package: helpers in /usr/lib.
    let pkg_machine = probe(&[PKG, STAGE]);
    assert!(pkg_machine.pkg_helper_available, "пакет: помощник в /usr/lib не найден");
    assert!(pkg_machine.stage_helper_available, "пакет: помощник в /usr/lib не найден");

    // A machine with a manual install: helpers in /usr/local/lib.
    let manual = probe(&[PKG_LOCAL, STAGE_LOCAL]);
    assert!(manual.pkg_helper_available, "ручная установка: помощник не найден");
    assert!(manual.stage_helper_available, "ручная установка: помощник не найден");

    // Both layouts at once — still "present"; none — an honest "no".
    let both = probe(&[PKG, STAGE, PKG_LOCAL, STAGE_LOCAL]);
    assert_eq!(both, pkg_machine);
    let bare = probe(&[]);
    assert!(!bare.pkg_helper_available && !bare.stage_helper_available, "{bare:?}");

    // The helpers are in place, but there is no pkexec on the system — nothing to ask for the password with.
    let seen = layout(&[PKG, STAGE]);
    let no_pkexec = ComplianceEnv::probe(PkgFamily::Arch, false, &seen, &seen);
    assert!(!no_pkexec.pkg_helper_available && !no_pkexec.stage_helper_available);

    // The helper is there, but its policy is not — there is no privileged path.
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
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn route_applies_then_checks() {
    let _lang = i18n_pin::ru();
    let io = kde_io();
    let (code, printed, _) = run_route(&io, "KDE");

    assert_eq!(code, 0);
    // First the whole switcher output, then the completeness check.
    assert_eq!(printed[0], "🖥️ Текущий рабочий стол: kde");
    let check_at = printed
        .iter()
        .position(|l| l.starts_with("🔍 Проверка рабочего стола"))
        .expect("проверка комплектности в выводе");
    assert!(check_at >= 3, "проверка идёт ПОСЛЕ применения: {printed:?}");
    assert!(printed.last().unwrap().starts_with("✅ Проверка завершена"), "{printed:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn route_returns_one_when_desktop_unknown() {
    let _lang = i18n_pin::ru();
    let io = FakeIo::new();
    let (code, printed, _) = run_route(&io, "");
    assert_eq!(code, 1);
    assert_eq!(printed[0], "⚠️ Не удалось определить текущий рабочий стол");
}

// ============================================================================
// GUARD
// ============================================================================

/// Guard: the route writes ONLY to the fake XDG directories.
///
/// The live machine does not change during the check — that is a condition of the task, not a wish:
/// the live session's default applications and system directories must not
/// be touched. The bite was tested: give the executor a system path
/// (for example, return `/usr/local/lib/rexon/...` to the autostart) and the check fails.
#[test]
fn guard_route_writes_only_under_fake_xdg_dirs() {
    let mut io = kde_io();
    io.rexon_path = None; // the most dangerous branch: the autostart fallback path
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

/// Guard: at login the route launches NOTHING privileged
/// and does not go to the network.
///
/// The environment is MATE ON PURPOSE: it is the one whose repair is privileged —
/// installing the missing ayatana packages under `pkexec`, an AUR helper, `git clone`
/// of panel layouts and copying them into a system directory. On the fake
/// machine those packages are absent, i.e. the repair branch is reachable, and the whole question is in
/// the `allow_privileged = false` gate. The fake executor of the check also
/// fails the check on any attempt to write or use a temporary directory. The bite
/// was tested: replacing the gate with `true` in [`apply_defaults_route`] fails the check
/// on `pkexec pacman` in the command list.
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
    // The line about deferred repair — no more than one per run.
    let deferred = printed.iter().filter(|l| **l == privileged_deferred_note()).count();
    assert!(deferred <= 1, "строка об отложенном лечении не должна повторяться");
}

/// A LIVE check of OBSERVATION (changes nothing and stops nobody): ask
/// the real session bus who holds `org.freedesktop.FileManager1`, and
/// make sure the parser understands it.
///
/// Why separately from the fake checks: `busctl status` prints `Comm`,
/// `Exe` and `CommandLine` not on all systemd versions, and if a field disappears,
/// the decision silently slides into the cautious "leave alone" branch — the defect comes back, while
/// the checks stay green. Here it is read-only: the test launches no command except
/// `busctl status`.
#[test]
#[cfg_attr(
    feature = "isolated-checks",
    ignore = "needs a live machine: queries the real session bus"
)]
fn live_bus_owner_of_filemanager1_is_understood() {
    use rexon_core::appdefaults as ad;
    use rexon_core::appdefaults_exec::LiveAppDefaultsIo;

    let io = LiveAppDefaultsIo;
    let Some(res) = io.run(&ad::busctl_status_cmd(ad::FM1_NAME)) else {
        return; // no session bus (CI) — nothing to check
    };
    if !res.ok || !res.stdout.contains("PID=") {
        return; // nobody holds the name — also a legitimate outcome
    }
    let facts = ad::busctl_owner(&res.stdout).expect("вывод шины обязан разбираться");
    assert!(facts.pid > 0, "{}", res.stdout);
    assert!(
        !facts.exe.is_empty() || !facts.comm.is_empty(),
        "шина перестала называть владельца — решение о слоте станет слепым: {}",
        res.stdout
    );
    // The owner's live machine: the name is held by a file manager, and it is recognized.
    if let Some(short) = ad::fm_short_from_process(&facts.comm, &facts.exe) {
        assert!(ad::fm_daemon_info(&short).is_some(), "нет записи в FM_DAEMONS: {short}");
    }
}

// ============================================================================
// English interface
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// The switcher log in English — without Russian letters, in phrases.
#[test]
fn english_switcher_log_has_no_russian() {
    let _lang = i18n_pin::en();
    let io = kde_io();
    let (ok, logs) = DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert!(ok);
    assert_eq!(logs[0], "🖥️ Current desktop: kde");
    assert_eq!(logs[2], "📝 Created kde-xdg-terminals.list");
    assert_eq!(logs[3], "✅ KDE terminal: konsole");

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

/// The 08.11.2026 case: Deepin brought in fcitx5 and its helper, their autostart
/// without `OnlyShowIn=` started in MATE and hijacked the keyboard layouts. Logging into the session
/// creates user copies with `OnlyShowIn=Deepin;DDE;`.
#[test]
fn login_confines_deepin_input_method_autostart() {
    let helper = "/etc/xdg/autostart/fcitx5-helper.desktop";
    let fcitx = "/etc/xdg/autostart/org.fcitx.Fcitx5.desktop";
    let entry = "[Desktop Entry]\nType=Application\nExec=fcitx5 -d\n";
    let io = mate_io()
        .file(helper, "[Desktop Entry]\nType=Application\nExec=fcitx5-helper\n")
        .file(fcitx, entry)
        .tool("pacman")
        .reply("pacman", true, "fcitx5\n");
    let sw = DeAppSwitcher::new(&io, env_for("MATE"));
    sw.apply_for_current_de();
    for name in ["fcitx5-helper.desktop", "org.fcitx.Fcitx5.desktop"] {
        let text = io.content(&format!("{CFG}/autostart/{name}")).unwrap_or_default();
        assert!(text.contains("OnlyShowIn=Deepin;DDE;"), "{name}: {text}");
        assert!(text.contains("X-Rexon-Confined=true"), "{name}: {text}");
    }
    // A second login rewrites nothing: the copy is already ours.
    let before = io.written_paths().len();
    sw.apply_for_current_de();
    let again: Vec<String> = io.written_paths().into_iter().skip(before).filter(|p| p.contains("/autostart/")).collect();
    assert!(again.is_empty(), "второй вход переписал: {again:?}");
}

/// fcitx5 installed by the person THEMSELVES (not as a dependency) is not restricted:
/// it is their input method on all desktops. The Deepin helper is restricted.
#[test]
fn login_keeps_input_method_installed_on_purpose() {
    let io = mate_io()
        .file("/etc/xdg/autostart/fcitx5-helper.desktop", "[Desktop Entry]\nExec=fcitx5-helper\n")
        .file("/etc/xdg/autostart/org.fcitx.Fcitx5.desktop", "[Desktop Entry]\nExec=fcitx5 -d\n")
        .tool("pacman")
        .reply("pacman", false, "");
    let sw = DeAppSwitcher::new(&io, env_for("MATE"));
    sw.apply_for_current_de();
    assert!(io.content(&format!("{CFG}/autostart/fcitx5-helper.desktop")).is_some());
    assert!(io.content(&format!("{CFG}/autostart/org.fcitx.Fcitx5.desktop")).is_none());
}

/// The person's own copy and an already restricted system entry are not touched.
#[test]
fn confined_autostart_respects_existing_choices() {
    use rexon_core::appdefaults::{self as ad, ConfinedCopy};
    let only = "Deepin;DDE;";
    // The person's own copy (no mark) — never touched, whatever the system has.
    let mine = "[Desktop Entry]\nHidden=true\n";
    assert_eq!(ad::confined_copy_action(Some("[Desktop Entry]\nExec=x\n"), Some(mine), only), ConfinedCopy::Keep);
    assert_eq!(ad::confined_copy_action(None, Some(mine), only), ConfinedCopy::Keep);
    // A record that restricts itself needs no copy.
    assert_eq!(ad::confined_copy_action(Some("[Desktop Entry]\nOnlyShowIn=Deepin;\n"), None, only), ConfinedCopy::Keep);
    let text = ad::confined_autostart_text("[Desktop Entry]\nNotShowIn=KDE;\nExec=x\n", only);
    assert!(!text.contains("NotShowIn"));
    assert!(text.ends_with("OnlyShowIn=Deepin;DDE;\nX-Rexon-Confined=true\n"));
    // COSMIC does not read `Hidden`: the restriction is OnlyShowIn only.
    let t = ad::confined_autostart_text("[Desktop Entry]\nExec=x\nHidden=false\n", only);
    assert!(!t.contains("Hidden"), "{t}");
}

/// Our confined copy follows the system record: created, refreshed after a
/// package update, removed when the package is gone or the record restricts
/// itself (step 0.6 of the desktop-protection plan).
#[test]
fn confined_copy_four_cases() {
    use rexon_core::appdefaults::{self as ad, ConfinedCopy};
    let only = "Deepin;DDE;";
    let v1 = "[Desktop Entry]\nExec=fcitx5 -d\n";
    let v2 = "[Desktop Entry]\nExec=fcitx5 -d --replace\n";
    let ours_v1 = ad::confined_autostart_text(v1, only);
    // none yet → write
    assert_eq!(ad::confined_copy_action(Some(v1), None, only), ConfinedCopy::Write(ours_v1.clone()));
    // up to date → keep
    assert_eq!(ad::confined_copy_action(Some(v1), Some(&ours_v1), only), ConfinedCopy::Keep);
    // stale after an update → rewrite
    assert_eq!(
        ad::confined_copy_action(Some(v2), Some(&ours_v1), only),
        ConfinedCopy::Write(ad::confined_autostart_text(v2, only))
    );
    // package removed → remove ours
    assert_eq!(ad::confined_copy_action(None, Some(&ours_v1), only), ConfinedCopy::Remove);
    // the record restricts itself now → ours is not needed
    assert_eq!(
        ad::confined_copy_action(Some("[Desktop Entry]\nExec=x\nOnlyShowIn=DDE;\n"), Some(&ours_v1), only),
        ConfinedCopy::Remove
    );

    // The executor: login removes our copy once the package is gone.
    let io = kde_io().file(&format!("{CFG}/autostart/fcitx5-helper.desktop"), &ours_v1);
    DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert!(io.content(&format!("{CFG}/autostart/fcitx5-helper.desktop")).is_none());
}

/// «Switch desktop» no longer throws away what the person chose in that
/// desktop: an installed program in the list wins, missing types are added.
#[test]
fn switch_keeps_choice_in_desktop_list() {
    let io = kde_io()
        .desktop("gimp.desktop")
        .file(&format!("{CFG}/kde-mimeapps.list"), "[Default Applications]\nimage/jpeg=gimp.desktop\ntext/plain=gone.desktop\n");
    let sw = DeAppSwitcher::new(&io, env_for("KDE"));
    assert!(matches!(sw.generate_profile("kde"), GenerateOutcome::Wrote(_)));
    let text = io.content(&format!("{CFG}/kde-mimeapps.list")).unwrap();
    assert!(text.contains("\nimage/jpeg=gimp.desktop\n"), "{text}");
    assert!(text.contains("\ntext/plain=org.kde.kate.desktop\n"), "a removed program is replaced: {text}");
    assert!(text.contains("\nimage/png=org.kde.gwenview.desktop\n"), "{text}");
}

/// A choice the person made during a session (the shared `mimeapps.list`
/// changed) goes into THAT session's list at the next login, whatever desktop
/// the next login is.
#[test]
fn login_carries_last_session_choice_into_its_list() {
    let ours = rexon_core::defaults::serialize_mimeapps_list(
        &[("image/jpeg".into(), "xviewer.desktop".into()), ("video/mp4".into(), "xplayer.desktop".into())],
        "ts",
    );
    let io = kde_io()
        .file(&format!("{CFG}/kde-mimeapps.list"), "[Default Applications]\n")
        .file(&format!("{CFG}/x-cinnamon-mimeapps.list"), &ours)
        .file(&format!("{CFG}/mimeapps.list"), "[Default Applications]\nimage/jpeg=xviewer.desktop\n");
    let sw = DeAppSwitcher::new(&io, env_for("X-Cinnamon"));
    // First login: nothing to compare with — only remembered.
    sw.apply_for_current_de();
    assert_eq!(io.content(&format!("{CFG}/x-cinnamon-mimeapps.list")).as_deref(), Some(ours.as_str()));
    let seen = io.content(&format!("{CFG}/rexon/mimeapps_seen.json")).unwrap();
    assert!(seen.contains("\"x-cinnamon-mimeapps.list\""), "{seen}");
    // During the Cinnamon session the person picks gThumb in Nemo and Firefox for pages.
    io.write_text(
        &format!("{CFG}/mimeapps.list"),
        "[Default Applications]\nimage/jpeg=org.gnome.gThumb.desktop\ntext/html=firefox.desktop\n",
    )
    .unwrap();
    // The next login is into Plasma: the choice goes into the Cinnamon list.
    DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    let cin = io.content(&format!("{CFG}/x-cinnamon-mimeapps.list")).unwrap();
    assert!(cin.contains("\nimage/jpeg=org.gnome.gThumb.desktop\n"), "{cin}");
    assert!(cin.contains("\nvideo/mp4=xplayer.desktop\n"), "{cin}");
    assert!(!cin.contains("text/html"), "the browser is never written: {cin}");
    // The Plasma list is not touched by a Cinnamon choice.
    assert!(!io.content(&format!("{CFG}/kde-mimeapps.list")).unwrap().contains("gThumb"));
}

/// labwc: `default=wlr;*` would give the file chooser and the colour scheme to
/// COSMIC; our user file keeps the system lines and sends them to GTK.
#[test]
fn labwc_portals_go_to_gtk() {
    use rexon_core::appdefaults::{self as ad, ConfinedCopy};
    let sys = "[preferred]\ndefault=wlr;*\norg.freedesktop.impl.portal.Inhibit=none\n";
    let ConfinedCopy::Write(text) = ad::labwc_portals_action(Some(sys), None, true, true) else {
        panic!("a file is needed");
    };
    assert!(text.starts_with("# Rexon: portals\n"), "{text}");
    for l in [
        "\n[preferred]\n",
        "\ndefault=wlr;*\n",
        "\norg.freedesktop.impl.portal.Inhibit=none\n",
        "\norg.freedesktop.impl.portal.FileChooser=gtk;\n",
        "\norg.freedesktop.impl.portal.Settings=gtk;\n",
        "\norg.freedesktop.impl.portal.AppChooser=gtk;\n",
    ] {
        assert!(text.contains(l), "{l:?} in {text}");
    }
    assert_eq!(ad::labwc_portals_action(Some(sys), Some(&text), true, true), ConfinedCopy::Keep);
    // The person's own file stays; without labwc, GTK portal or `*` ours goes.
    assert_eq!(ad::labwc_portals_action(Some(sys), Some("[preferred]\ndefault=gtk\n"), true, true), ConfinedCopy::Keep);
    assert_eq!(ad::labwc_portals_action(Some(sys), Some(&text), false, true), ConfinedCopy::Remove);
    assert_eq!(ad::labwc_portals_action(Some(sys), None, true, false), ConfinedCopy::Keep);
    assert_eq!(ad::labwc_portals_action(Some("[preferred]\ndefault=wlr;gtk\n"), Some(&text), true, true), ConfinedCopy::Remove);

    // The executor writes it at login.
    let io = kde_io()
        .tool("labwc")
        .file("/usr/share/xdg-desktop-portal/labwc-portals.conf", sys)
        .file("/usr/share/xdg-desktop-portal/portals/gtk.portal", "[portal]\n");
    DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert_eq!(io.content(&format!("{CFG}/xdg-desktop-portal/labwc-portals.conf")).as_deref(), Some(text.as_str()));
}

/// Login moves Rexon's Cinnamon list from the unread `cinnamon-mimeapps.list`
/// to `x-cinnamon-mimeapps.list` (what GLib reads under `X-Cinnamon`); a file
/// of the person under the old name stays.
#[test]
fn login_moves_misnamed_cinnamon_list() {
    let ours = rexon_core::defaults::serialize_mimeapps_list(
        &[("image/jpeg".into(), "xviewer.desktop".into())],
        "2026-11-25T00:09:01",
    );
    let io = kde_io().file(&format!("{CFG}/cinnamon-mimeapps.list"), &ours);
    DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert!(io.content(&format!("{CFG}/cinnamon-mimeapps.list")).is_none());
    assert_eq!(io.content(&format!("{CFG}/x-cinnamon-mimeapps.list")).as_deref(), Some(ours.as_str()));

    let mine = "[Default Applications]\nimage/jpeg=gimp.desktop\n";
    let io = kde_io().file(&format!("{CFG}/cinnamon-mimeapps.list"), mine);
    DeAppSwitcher::new(&io, env_for("KDE")).apply_for_current_de();
    assert_eq!(io.content(&format!("{CFG}/cinnamon-mimeapps.list")).as_deref(), Some(mine));
    assert!(io.content(&format!("{CFG}/x-cinnamon-mimeapps.list")).is_none());
}
