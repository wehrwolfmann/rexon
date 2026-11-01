//! Накладка настроек виджета Conky ([`rexon_core::conky_overlay`]):
//! золотые файлы накладки, чтение значений автора, запуск через накладку,
//! автозапуск и перезапуск запущенного виджета на подставном conky.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use rexon_core::animations::LookIo;
use rexon_core::conky_overlay::{self as ov, Env, Overrides, Value};
use rexon_core::de_apply::Ctx;

/// Подставная машина: файлы в памяти, команды записываются.
#[derive(Default)]
struct Fake {
    files: RefCell<HashMap<String, String>>,
    ran: RefCell<Vec<Vec<String>>>,
    version: Option<&'static str>,
}

impl LookIo for Fake {
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.ran.borrow_mut().push(argv.to_vec());
        match (argv.first().map(String::as_str), argv.get(1).map(String::as_str), self.version) {
            (Some("conky"), Some("-v"), Some(v)) => (true, format!("conky {v} compiled for Linux x86_64\n")),
            _ => (false, String::new()),
        }
    }
    fn have(&self, _: &str) -> bool {
        true
    }
    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        self.files.borrow_mut().insert(path.to_string(), text.to_string());
        true
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut out: Vec<String> = self
            .files
            .borrow()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|r| r.split('/').next().unwrap_or(r).to_string()))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

fn ctx() -> Ctx {
    Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: "mate".into() }
}

const AUTHOR: &str = "\
-- Виджет автора
conky.config = {
    alignment = 'tr',
    gap_x = 15,
    gap_y = 40,
    minimum_width = 200,
    own_window = true,
    own_window_transparent = false,
    own_window_argb_visual = true,
    own_window_argb_value = 128,
    own_window_colour = '1d1f21',
    update_interval = 2.0,
}
conky.text = [[
${time %H:%M}
]]
";

#[test]
fn golden_overlay_modern_conky() {
    let cfg = ov::parse_config(AUTHOR).unwrap();
    let o = Overrides {
        alignment: Some("bottom_left".into()),
        gap_x: Some(30),
        gap_y: Some(-4),
        min_width: Some(250),
        min_height: Some(100),
        bg_opacity: Some(80),
        update_interval: Some(0.5),
        xinerama_head: Some(1),
        ..Default::default()
    };
    let env = Env { version: Some((1, 25)), de: "mate".into() };
    let got = ov::render("/h/.config/conky/W/conky.conf", &cfg, &o, &env);
    assert_eq!(
        got,
        "\
-- Rexon: the widget's settings on top of the author's file.
-- The author's file stays untouched; Rexon rewrites this file on every change.
dofile(\"/h/.config/conky/W/conky.conf\")
conky.config = conky.config or {}
local c = conky.config
c.alignment = \"bottom_left\"
c.gap_x = 30
c.gap_y = -4
c.minimum_width = 250
c.minimum_height = 100
c.own_window = true
c.own_window_transparent = nil
c.own_window_argb_value = nil
c.own_window_colour = nil
c.own_window_color = \"#CC1D1F21\"
c.update_interval = 0.5
c.xinerama_head = 1
"
    );
}

/// conky до 1.23: прозрачность — прежними полями, цвет — без альфы.
#[test]
fn golden_overlay_old_conky_background() {
    let cfg = ov::parse_config(AUTHOR).unwrap();
    let o = Overrides { bg_none: Some(true), bg_colour: Some("FF0000".into()), ..Default::default() };
    let env = Env { version: Some((1, 19)), de: "xfce".into() };
    let got = ov::render("/w/a b.conf", &cfg, &o, &env);
    assert!(got.contains("dofile(\"/w/a b.conf\")\n"), "{got}");
    assert!(got.ends_with(
        "c.own_window = true\n\
         c.own_window_transparent = true\n\
         c.own_window_argb_visual = true\n\
         c.own_window_argb_value = 0\n\
         c.own_window_color = nil\n\
         c.own_window_colour = \"FF0000\"\n"
    ), "{got}");
}

#[test]
fn current_values_author_then_own() {
    let cfg = ov::parse_config(AUTHOR).unwrap();
    let cur = ov::current(&cfg, &Overrides::default());
    assert_eq!(cur.alignment.value, "top_right");
    assert!(!cur.alignment.changed);
    assert_eq!((cur.gap_x.value, cur.gap_y.value, cur.min_width.value), (15, 40, 200));
    assert_eq!(cur.bg_opacity.value, 50);
    assert!(!cur.bg_none.value);
    assert_eq!(cur.bg_colour.value, "1D1F21");
    assert_eq!(cur.update_interval.value, 2.0);
    assert_eq!(cur.xinerama_head.value, -1);

    let o = Overrides { gap_x: Some(99), bg_none: Some(true), ..Default::default() };
    let cur = ov::current(&cfg, &o);
    assert!(cur.gap_x.changed && cur.gap_x.value == 99);
    assert!(cur.bg_none.value && cur.bg_none.changed);

    // Выражение у автора — поле заперто, пока человек не задаст своё.
    let expr = ov::parse_config("conky.config = { gap_x = 10 * 2, own_window_argb_value = a }").unwrap();
    assert_eq!(expr["gap_x"], Value::Expr);
    let cur = ov::current(&expr, &Overrides::default());
    assert!(cur.gap_x.locked && cur.bg_opacity.locked);
    assert!(!ov::current(&expr, &Overrides { gap_x: Some(3), ..Default::default() }).gap_x.locked);

    // Альфа в самом цвете (conky ≥ 1.23).
    let modern = ov::parse_config("conky.config = { own_window_color = '#33000000' }").unwrap();
    assert_eq!(ov::current(&modern, &Overrides::default()).bg_opacity.value, 20);
}

#[test]
fn launch_goes_through_overlay_only_when_changed() {
    let io = Fake { version: Some("1.25.2"), ..Default::default() };
    let conf = "/h/.config/conky/W/conky.conf";
    io.write(conf, AUTHOR);
    let p = ov::paths(&ctx(), None, conf);
    assert_eq!(ov::launch_conf(&io, &ctx(), &p, conf), conf);
    assert!(io.read(&p.overlay).is_none());

    let o = Overrides { gap_x: Some(1), ..Default::default() };
    ov::save(&io, &p, conf, &o, &Env::default()).unwrap();
    assert_eq!(ov::load(&io, &p), o);
    // Запуск пишет накладку заново (файл автора мог обновиться).
    io.write(&p.overlay, "stale");
    assert_eq!(ov::launch_conf(&io, &ctx(), &p, conf), p.overlay);
    let text = io.read(&p.overlay).unwrap();
    assert!(text.contains(&format!("dofile(\"{conf}\")")) && text.contains("c.gap_x = 1\n"), "{text}");

    // Процесс с накладкой — тот же виджет.
    let cmd = format!("conky\0-d\0-c\0{}\0", p.overlay);
    assert_eq!(rexon_core::de_apply::conky_conf_of_cmdline(&cmd).as_deref(), Some(conf));
    assert_eq!(rexon_core::de_apply::conky_launch_of_cmdline(&cmd).as_deref(), Some(p.overlay.as_str()));
}

#[test]
fn processes_tell_overlay_from_author_file() {
    let io = Fake::default();
    let conf = "/h/.config/conky/W/conky.conf";
    let p = ov::paths(&ctx(), None, conf);
    io.write("/proc/10/cmdline", &format!("conky\0-c\0{conf}\0"));
    io.write("/proc/11/cmdline", &format!("/usr/bin/conky\0-d\0-c\0{}\0", p.overlay));
    io.write("/proc/12/cmdline", "conky\0-c\0/h/other.conf\0");
    let got = ov::processes_of(&io, conf);
    assert_eq!(got, vec![("10".to_string(), false), ("11".to_string(), true)]);
}

#[test]
fn autostart_follows_overlay() {
    let conf = "/h/.config/conky/My W/conky.conf";
    let plain = rexon_core::ext_manager::conky_desktop_entry("My W", conf);
    let via = rexon_core::ext_manager::conky_desktop_entry_via("My W", conf, Some("/s/o.rexon.conf"));
    assert!(via.contains(&format!(
        "\nExec=sh -c \"cd \\\\\"\\\\$(dirname \\\\\"\\\\$1\\\\\")\\\\\" && exec conky -c \\\\\"\\\\$0\\\\\"\" /s/o.rexon.conf \"{conf}\"\n"
    )), "{via}");
    assert!(via.contains("\nPath=/h/.config/conky/My W\n"), "{via}");

    // Включённый автозапуск переписывается при правке и при сбросе.
    let io = Fake { version: Some("1.25.2"), ..Default::default() };
    io.write(conf, AUTHOR);
    let entry = "/h/.config/autostart/rexon-conky-My W.desktop";
    io.write(entry, &plain);
    io.write("/h/.config/autostart/rexon-conky-other.desktop", "[Desktop Entry]\nExec=sh -c x /h/o.conf\n");
    let p = ov::paths(&ctx(), None, conf);
    let (ok, _) = ov::apply(&io, &ctx(), &p, "My W", conf, &Overrides { gap_y: Some(5), ..Default::default() });
    assert!(ok);
    assert!(io.read(entry).unwrap().contains(&p.overlay));
    assert_eq!(io.read("/h/.config/autostart/rexon-conky-other.desktop").unwrap(), "[Desktop Entry]\nExec=sh -c x /h/o.conf\n");
    let (ok, _) = ov::reset(&io, &ctx(), &p, "My W", conf);
    assert!(ok);
    assert_eq!(io.read(entry).unwrap(), plain);
}

#[test]
fn alignment_names() {
    assert_eq!(ov::normalize_alignment("TR"), Some("top_right"));
    assert_eq!(ov::normalize_alignment("center"), Some("middle_middle"));
    assert_eq!(ov::normalize_alignment("none"), None);
    assert_eq!(ov::ALIGNMENTS.len(), 9);
}

/// Настоящие виджеты магазина (архивы раздела 124, скачанные для
/// исследования в `~/research/conky-samples`): таблица `conky.config`
/// разбирается, положение — литералами. Нет образцов — проверка пропускается.
#[test]
fn real_store_widgets_parse() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let base = std::path::Path::new(&home).join("research/conky-samples");
    let samples = [
        "1384943/x/conkyrc_grey",
        "1955473/x/Regulus/Regulus.conf",
        "1961175/x/Alterf/Alterf.conf",
        "1869486/x/Mimosa/Mimosa.conf",
        "1607304/x/conky/nordcore/conkyrc2core",
        "1170490/x/AutomatiK/clockfile",
        "1296257/x/conky-grapes-master/conky_tpl",
    ];
    let mut seen = 0;
    for s in samples {
        let Ok(text) = std::fs::read_to_string(base.join(s)) else { continue };
        seen += 1;
        let cfg: BTreeMap<String, Value> = ov::parse_config(&text).unwrap_or_else(|| panic!("{s}: нет conky.config"));
        assert!(cfg.len() >= 5, "{s}: {cfg:?}");
        let cur = ov::current(&cfg, &Overrides::default());
        assert!(!cur.gap_x.locked && !cur.alignment.locked, "{s}: {cfg:?}");
    }
    if seen == 0 {
        eprintln!("НЕ ПРОВЕРЕНО: нет образцов виджетов в {}", base.display());
    }
}

/// Стенд на подставном conky (`tools/de_matrix_stand/fake_conky.c` за
/// сценарием, который отвечает на `-v`): виджет, запущенный с файлом
/// автора, после правки перезапускается с накладкой — рабочий каталог —
/// папка автора; после «Сбросить» — снова с файлом автора. Гасятся только
/// процессы стенда.
#[test]
fn running_widget_switches_to_overlay_and_back() {
    use rexon_core::animations::SystemIo;
    let dir = std::env::temp_dir().join(format!("rexon-conky-overlay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let real = dir.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/de_matrix_stand/fake_conky.c");
    let built = std::process::Command::new("cc").arg("-O2").arg("-o").arg(real.join("conky")).arg(&src).status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("НЕ ПРОВЕРЕНО: нет компилятора C для подставного conky");
        return;
    }
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in -v) echo 'conky 1.25.2 compiled for Linux'; exit 0;; esac\nexec '{}' \"$@\"\n",
        real.join("conky").display()
    );
    std::fs::write(bin.join("conky"), script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin.join("conky"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    std::env::set_var("XDG_STATE_HOME", dir.join("state"));

    let home = dir.join("home");
    let conf = home.join(".config/conky/W/conky.conf").to_string_lossy().into_owned();
    std::fs::create_dir_all(home.join(".config/conky/W")).unwrap();
    std::fs::write(&conf, AUTHOR).unwrap();
    let ctx = Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: "mate".into() };
    let io = SystemIo;
    rexon_core::i18n::set_language(Some("en"));

    let (ok, msg) = rexon_core::conky_procs::start_conf(&io, &ctx, "W", &conf);
    assert!(ok, "{msg}");
    let first = ov::processes_of(&io, &conf);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(!first[0].1, "без правок — с файлом автора");

    let p = ov::live_paths(&ctx, &conf);
    let (ok, msg) = ov::apply(&io, &ctx, &p, "W", &conf, &Overrides { gap_x: Some(77), ..Default::default() });
    let now = ov::processes_of(&io, &conf);
    let cleanup = |procs: &[(String, bool)]| {
        for (pid, _) in procs {
            let _ = std::process::Command::new("kill").arg(pid).status();
        }
    };
    if !ok || now.len() != 1 || !now[0].1 {
        cleanup(&now);
        cleanup(&first);
        panic!("после правки — один процесс с накладкой: {ok} {msg} {now:?}");
    }
    assert!(std::fs::read_to_string(&p.overlay).unwrap().contains("c.gap_x = 77\n"));
    let cwd = std::fs::read_link(format!("/proc/{}/cwd", now[0].0)).unwrap();
    assert_eq!(cwd, home.join(".config/conky/W"));
    // Сопоставление «запущен ли виджет» видит его по файлу автора.
    assert!(rexon_core::conky_procs::running_confs(&io, &ctx).contains(&conf));

    let (ok, msg) = ov::reset(&io, &ctx, &p, "W", &conf);
    let back = ov::processes_of(&io, &conf);
    cleanup(&back);
    assert!(ok, "{msg}");
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(!back[0].1, "после «Сбросить» — снова с файлом автора");
    assert!(!std::path::Path::new(&p.overlay).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Синтаксис 1.9 ─────────────────────────────────────────────────────────

/// Пары «файл 1.9 → перевод в 1.10».
#[test]
fn converts_19_files() {
    let cases: &[(&str, &str)] = &[
        // 1. Обычная шапка: да/нет, числа, строки с пробелами, цвета из цифр.
        (
            "# comment\nbackground yes\nupdate_interval 1.5\nalignment tr\nxftfont DejaVu Sans:size=8\n\
             color1 000000\ndefault_color white # trailing\nown_window_hints undecorated,below\n\nTEXT\n${time %H:%M}\n",
            "-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\nconky.config = {\n    \
             background = true,\n    update_interval = 1.5,\n    alignment = \"tr\",\n    font = \"DejaVu Sans:size=8\",\n    \
             color1 = \"000000\",\n    default_color = \"white\",\n    own_window_hints = \"undecorated,below\",\n}\n\
             conky.text = [[\n${time %H:%M}]]\n",
        ),
        // 2. Размер парой чисел — два поля; поле без значения — «да».
        (
            "minimum_size 190 450\ndefault_bar_size 30 8\nown_window\ngap_x -10\nTEXT\nx\n",
            "-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\nconky.config = {\n    \
             minimum_width = 190,\n    minimum_height = 450,\n    default_bar_width = 30,\n    default_bar_height = 8,\n    \
             own_window = true,\n    gap_x = -10,\n}\nconky.text = [[\nx]]\n",
        ),
        // 3. Скобки в тексте: уровень длинной строки — тот, что не закроется
        //    раньше времени.
        (
            "TEXT\n${if_match \"${a}\"==\"]]\"}x]] y]${endif}\n",
            "-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\nconky.config = {\n}\n\
             conky.text = [=[\n${if_match \"${a}\"==\"]]\"}x]] y]${endif}]=]\n",
        ),
        // 4. `\#` в настройке — знак, а не комментарий; кавычки снимаются,
        //    кавычка внутри экранируется.
        (
            "own_window_title \"Conky \\#1\"\ntemplate0 ${cpu \\1}\"\nTEXT\na\n\nb",
            "-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\nconky.config = {\n    \
             own_window_title = \"Conky #1\",\n    template0 = \"${cpu \\\\1}\\\"\",\n}\nconky.text = [[\na\n\nb]]\n",
        ),
        // 5. Переводы строк Windows и пробелы перед TEXT.
        (
            "gap_y 5\r\n  TEXT  \r\nline\r\n",
            "-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\nconky.config = {\n    \
             gap_y = 5,\n}\nconky.text = [[\nline\r]]\n",
        ),
    ];
    for (i, (old, new)) in cases.iter().enumerate() {
        assert_eq!(ov::syntax(old), ov::Syntax::V19, "случай {}", i + 1);
        let got = ov::convert_19(old);
        assert_eq!(&got, new, "случай {}", i + 1);
        assert_eq!(ov::syntax(&got), ov::Syntax::V110);
        assert!(ov::parse_config(&got).is_some(), "случай {}: перевод разбирается", i + 1);
    }
    assert_eq!(ov::syntax(AUTHOR), ov::Syntax::V110);
}

/// Накладка поверх виджета 1.9 выполняет его перевод, а не файл автора
/// (`dofile` старого синтаксиса падает), и видит его поля.
#[test]
fn overlay_over_19_runs_the_translated_copy() {
    let io = Fake { version: Some("1.25.2"), ..Default::default() };
    let conf = "/h/.conky/Old/conkyrc";
    io.write(conf, "alignment tl\ngap_x 7\nminimum_size 100 50\nTEXT\nhi\n");
    let p = ov::paths(&ctx(), None, conf);
    let (syn, cfg) = ov::author_config(&io.read(conf).unwrap());
    assert_eq!(syn, ov::Syntax::V19);
    let cur = ov::current(&cfg.unwrap(), &Overrides::default());
    assert_eq!((cur.alignment.value.as_str(), cur.gap_x.value, cur.min_height.value), ("top_left", 7, 50));

    ov::save(&io, &p, conf, &Overrides { gap_x: Some(8), ..Default::default() }, &Env::default()).unwrap();
    let overlay = io.read(&p.overlay).unwrap();
    assert!(overlay.contains(&format!("dofile(\"{}\")", p.converted)), "{overlay}");
    assert!(io.read(&p.converted).unwrap().contains("conky.text = [[\nhi]]"));
    // Файл автора не тронут.
    assert_eq!(io.read(conf).unwrap(), "alignment tl\ngap_x 7\nminimum_size 100 50\nTEXT\nhi\n");
}

/// «Окно — как советует Rexon»: ветки Wayland и X11 в накладке, сеанс
/// узнаётся при запуске conky.
#[test]
fn golden_overlay_window_preset() {
    let cfg = ov::parse_config(AUTHOR).unwrap();
    let o = Overrides { window_preset: Some(true), ..Default::default() };
    let tail = |de: &str, v: (u32, u32)| {
        let got = ov::render("/a.conf", &cfg, &o, &Env { version: Some(v), de: de.into() });
        got.split_once("local c = conky.config\n").unwrap().1.to_string()
    };
    assert_eq!(
        tail("kde", (1, 25)),
        "c.own_window = true\nif os.getenv(\"WAYLAND_DISPLAY\") then\n    c.own_window_type = \"normal\"\n    \
         c.own_window_hints = \"undecorated,below,sticky,skip_taskbar,skip_pager\"\n    c.own_window_namespace = \"desktop\"\n\
         else\n    c.own_window_type = \"normal\"\n    c.own_window_hints = \"undecorated,below,sticky,skip_taskbar,skip_pager\"\n    \
         c.own_window_namespace = nil\nend\n"
    );
    assert_eq!(
        tail("gnome", (1, 19)),
        "c.own_window = true\nc.own_window_argb_visual = true\nif os.getenv(\"WAYLAND_DISPLAY\") then\n    \
         -- no layer-shell here: the author's window settings stay\nelse\n    c.own_window_type = \"normal\"\n    \
         c.own_window_hints = \"undecorated,below,sticky,skip_taskbar,skip_pager\"\nend\n"
    );
}

// ── Экран, часы, сеть ─────────────────────────────────────────────────────

#[test]
fn monitors_only_on_x11() {
    struct X;
    impl LookIo for X {
        fn run(&self, argv: &[String]) -> (bool, String) {
            assert_eq!(argv, ["xrandr", "--listmonitors"]);
            (true, "Monitors: 1\n 0: +*eDP 1920/355x1080/199+0+0  eDP\n".into())
        }
        fn have(&self, t: &str) -> bool {
            t == "xrandr"
        }
        fn read(&self, _: &str) -> Option<String> {
            None
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }
    let m = ov::monitors(&X, false);
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].name.as_str(), m[0].width, m[0].primary), ("eDP", 1920, true));
    assert!(ov::monitors(&X, true).is_empty());
    assert!(ov::monitor_note(true).is_some() && ov::monitor_note(false).is_none());
}

const NET_TEXT: &str = "conky.config = {}\nconky.text = [[\n\
${time %H:%M} ${tztime Europe/Berlin %k:%M}\n\
${if_up eth0}LAN ${addr eth0} ${downspeedgraph eth0 20,100 000000 ffffff}${endif}\n\
${if_up wlan0}WiFi ${wireless_essid wlan0} ${upspeedf wlan0}${endif}\n\
${color #eth0}eth0 stays in text ${exec echo eth0}\n]]\n";

#[test]
fn clock_and_network_found_in_text() {
    assert_eq!(ov::clock_style(NET_TEXT), Some(true));
    assert_eq!(ov::clock_style("${time %I:%M %p}"), Some(false));
    assert_eq!(ov::clock_style("${time %A %d}"), None);
    assert_eq!(ov::net_names(NET_TEXT), ["eth0", "wlan0"]);

    let io = Fake::default();
    io.write("/sys/class/net/lo/uevent", "INTERFACE=lo\n");
    io.write("/sys/class/net/wlan0/uevent", "DEVTYPE=wlan\nINTERFACE=wlan0\n");
    io.write("/sys/class/net/wlan0/device/uevent", "DRIVER=rtw89\n");
    io.write("/sys/class/net/eno1/uevent", "INTERFACE=eno1\n");
    io.write("/sys/class/net/eno1/device/uevent", "DRIVER=r8169\n");
    io.write("/sys/class/net/docker0/uevent", "DEVTYPE=bridge\n");
    let got: Vec<(String, &str)> = ov::interfaces(&io).into_iter().map(|i| (i.name, i.kind)).collect();
    assert_eq!(got, [("wlan0".to_string(), "wifi"), ("eno1".into(), "ethernet"), ("docker0".into(), "virtual")]);
}

/// Замены в тексте работают в настоящем Lua (если он есть): каждое имя —
/// своей заменой (обмен eth0 ↔ wlan0 не сливает их), только внутри
/// сетевых переменных; часы 24 → 12 и обратно.
#[test]
fn text_rewrites_run_in_real_lua() {
    let Some(lua) = ["lua5.4", "lua", "lua5.3"].iter().find(|l| {
        std::process::Command::new(l).arg("-v").output().is_ok_and(|o| o.status.success())
    }) else {
        eprintln!("НЕ ПРОВЕРЕНО: нет интерпретатора Lua");
        return;
    };
    let dir = std::env::temp_dir().join(format!("rexon-conky-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let author = dir.join("w.conf");
    std::fs::write(&author, NET_TEXT).unwrap();
    let run = |o: &Overrides| -> String {
        let cfg = ov::parse_config(NET_TEXT).unwrap();
        let text = ov::render(&author.to_string_lossy(), &cfg, o, &Env::default());
        let file = dir.join("o.lua");
        std::fs::write(&file, format!("conky = {{}}\n{text}\nio.write(conky.text)\n")).unwrap();
        let out = std::process::Command::new(lua).arg(&file).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let mut nets = BTreeMap::new();
    nets.insert("eth0".to_string(), "wlan0".to_string());
    nets.insert("wlan0".to_string(), "eth0".to_string());
    let got = run(&Overrides { net_ifaces: nets, clock_24h: Some(false), ..Default::default() });
    assert_eq!(
        got,
        "${time %I:%M %p} ${tztime Europe/Berlin %l:%M %p}\n\
         ${if_up wlan0}LAN ${addr wlan0} ${downspeedgraph wlan0 20,100 000000 ffffff}${endif}\n\
         ${if_up eth0}WiFi ${wireless_essid eth0} ${upspeedf eth0}${endif}\n\
         ${color #eth0}eth0 stays in text ${exec echo eth0}\n"
    );
    let back = run(&Overrides { clock_24h: Some(true), ..Default::default() });
    assert!(back.starts_with("${time %H:%M} ${tztime Europe/Berlin %k:%M}\n"), "{back}");
    let twelve = "conky.text = [[${time %I:%M %p}|${time %r}]]";
    std::fs::write(&author, twelve).unwrap();
    let to24 = run(&Overrides { clock_24h: Some(true), ..Default::default() });
    assert_eq!(to24, "${time %H:%M}|${time %H:%M:%S}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Блок настроек карточки ────────────────────────────────────────────────

#[test]
fn settings_view_and_changes() {
    rexon_core::i18n::set_language(Some("en"));
    let io = Fake { version: Some("1.25.2"), ..Default::default() };
    let conf = "/h/.config/conky/W/conky.conf";
    io.write(conf, &(AUTHOR.replace("${time %H:%M}", "${time %H:%M} ${downspeed eth0}")));
    let p = ov::paths(&ctx(), None, conf);
    let m = ov::Machine {
        monitors: ov::parse_monitors(" 0: +*eDP 1920/355x1080/199+0+0  eDP\n 1: +HDMI 2560/1x1440/1+1920+0  HDMI\n"),
        interfaces: vec![ov::Iface { name: "wlan0".into(), kind: "wifi" }],
        compositor: Some(false),
    };
    let v = ov::settings_view(&io, &ctx(), &p, conf, &m);
    assert!(v.available && !v.changed);
    assert_eq!(v.alignments.len(), 9);
    assert_eq!(v.cur.as_ref().unwrap().alignment.value, "top_right");
    assert_eq!(v.clock, Some(true));
    assert_eq!(v.nets.len(), 1);
    assert_eq!(v.nets[0].from, "eth0");
    assert_eq!(v.nets[0].choices.len(), 2);
    assert_eq!(v.screen_choices.len(), 3);
    // Фон полупрозрачный, композитора нет — предупреждение.
    assert!(v.compositor_note.is_some());

    let mut o = Overrides::default();
    ov::set_value(&mut o, "alignment", &serde_json::json!("bl")).unwrap();
    ov::set_value(&mut o, "gap_x", &serde_json::json!(12)).unwrap();
    ov::set_value(&mut o, "min_width", &serde_json::json!(-5)).unwrap();
    ov::set_value(&mut o, "bg_none", &serde_json::json!(true)).unwrap();
    ov::set_value(&mut o, "bg_opacity", &serde_json::json!(40)).unwrap();
    ov::set_value(&mut o, "bg_colour", &serde_json::json!("#a0b0c0")).unwrap();
    ov::set_value(&mut o, "xinerama_head", &serde_json::json!(1)).unwrap();
    ov::set_value(&mut o, "net:eth0", &serde_json::json!("wlan0")).unwrap();
    assert_eq!(o.alignment.as_deref(), Some("bottom_left"));
    assert_eq!((o.gap_x, o.min_width), (Some(12), Some(0)));
    assert_eq!(o.bg_none, Some(false), "ползунок прозрачности возвращает фон");
    assert_eq!(o.bg_colour.as_deref(), Some("A0B0C0"));
    assert_eq!(o.xinerama_head, Some(1));
    assert_eq!(o.net_ifaces.get("eth0").map(String::as_str), Some("wlan0"));
    ov::set_value(&mut o, "xinerama_head", &serde_json::json!(-1)).unwrap();
    ov::set_value(&mut o, "net:eth0", &serde_json::json!("eth0")).unwrap();
    assert!(o.xinerama_head.is_none() && o.net_ifaces.is_empty());
    assert!(ov::set_value(&mut o, "nope", &serde_json::json!(1)).is_err());
    assert!(ov::set_value(&mut o, "net:eth0", &serde_json::json!("a b;rm")).is_err());
    assert!(ov::set_value(&mut o, "alignment", &serde_json::json!("sideways")).is_err());

    // Файл, который виджет не читает кодом, — настройки недоступны.
    io.write("/h/x.conf", "conky.config = make()\nconky.text = [[x]]\n");
    let p2 = ov::paths(&ctx(), None, "/h/x.conf");
    let v2 = ov::settings_view(&io, &ctx(), &p2, "/h/x.conf", &m);
    assert!(!v2.available && !v2.reason.is_empty());
}
