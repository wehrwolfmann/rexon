//! Светлое и тёмное по расписанию (`rexon_core::day_night`).
//!
//! * Солнце — проверочные точки по опубликованным таблицам восхода и
//!   заката (средние широты обоих полушарий, экватор, полярный круг);
//!   допуск — 3 минуты.
//! * Что применяется в каком окружении, тексты таймера и настройки Plasma —
//!   эталон `golden/day_night_cases.json` (переснять:
//!   `REXON_GOLDEN_WRITE=1 cargo test -p rexon-core --test day_night`).
//! * «Сохранить» и запуск по таймеру — на поддельной машине: какие файлы
//!   пишутся, какие команды зовутся, что применяется.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::day_night::*;
use serde_json::{json, Value};

const GOLDEN: &str = "tests/golden/day_night_cases.json";

fn at(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
    days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60
}

fn near(got: i64, want: i64, what: &str) {
    assert!((got - want).abs() <= 180, "{what}: {} против {} (разница {} с)", utc_calendar(got), utc_calendar(want), got - want);
}

fn normal(day: SunDay) -> (i64, i64) {
    match day {
        SunDay::Normal { rise, set } => (rise, set),
        other => panic!("ждали восход и закат, получили {other:?}"),
    }
}

/// Проверочные точки: (место, широта, долгота, полдень дня в UTC,
/// восход UTC, закат UTC). Время по таблицам местное, здесь — переведено в UTC.
#[test]
fn восход_и_закат_по_проверочным_точкам() {
    let cases: [(&str, f64, f64, i64, i64, i64); 6] = [
        // Берлин, солнцестояние: 04:43 и 21:33 CEST.
        ("Берлин 21.06", 52.52, 13.405, at(2026, 6, 21, 11, 0), at(2026, 6, 21, 2, 43), at(2026, 6, 21, 19, 33)),
        // Берлин, зимнее солнцестояние: 08:15 и 15:54 CET.
        ("Берлин 21.12", 52.52, 13.405, at(2026, 12, 21, 11, 0), at(2026, 12, 21, 7, 15), at(2026, 12, 21, 14, 54)),
        // Москва: 03:44 и 21:18 MSK.
        ("Москва 21.06", 55.7558, 37.6173, at(2026, 6, 21, 9, 30), at(2026, 6, 21, 0, 44), at(2026, 6, 21, 18, 18)),
        // Экватор, нулевой меридиан, равноденствие: полдень ≈ 12:07 UTC
        // (уравнение времени), день чуть длиннее 12 ч из-за рефракции.
        ("Экватор 20.03", 0.0, 0.0, at(2026, 3, 20, 12, 0), at(2026, 3, 20, 6, 4), at(2026, 3, 20, 18, 11)),
        // Сидней, южное лето: 05:41 и 20:05 AEDT (UTC+11) — восход ещё
        // 20 декабря по UTC.
        ("Сидней 21.12", -33.8688, 151.2093, at(2026, 12, 21, 1, 0), at(2026, 12, 20, 18, 41), at(2026, 12, 21, 9, 5)),
        // Нью-Йорк, равноденствие: 06:59 и 19:09 EDT (UTC−4).
        ("Нью-Йорк 20.03", 40.7128, -74.006, at(2026, 3, 20, 17, 0), at(2026, 3, 20, 10, 59), at(2026, 3, 20, 23, 9)),
    ];
    for (place, lat, lon, noon, rise, set) in cases {
        let (r, s) = normal(sun_around(noon, lat, lon));
        near(r, rise, &format!("{place}: восход"));
        near(s, set, &format!("{place}: закат"));
    }
}

#[test]
fn полярный_день_и_ночь() {
    let (lat, lon) = (69.6492, 18.9553); // Тромсё
    assert_eq!(sun_around(at(2026, 12, 21, 11, 0), lat, lon), SunDay::PolarNight);
    assert_eq!(sun_around(at(2026, 6, 21, 11, 0), lat, lon), SunDay::PolarDay);
    assert_eq!(phase_by_sun(at(2026, 12, 21, 11, 0), lat, lon), Phase::Dark);
    assert_eq!(phase_by_sun(at(2026, 6, 21, 23, 0), lat, lon), Phase::Light);
    assert!(next_sun_switches(at(2026, 6, 21, 11, 0), lat, lon, 2).is_empty());
}

#[test]
fn сейчас_и_следующие_смены_по_солнцу() {
    let (lat, lon) = (52.52, 13.405);
    assert_eq!(phase_by_sun(at(2026, 6, 21, 12, 0), lat, lon), Phase::Light);
    assert_eq!(phase_by_sun(at(2026, 6, 21, 22, 0), lat, lon), Phase::Dark);
    assert_eq!(phase_by_sun(at(2026, 6, 22, 1, 0), lat, lon), Phase::Dark);
    let next = next_sun_switches(at(2026, 6, 21, 12, 0), lat, lon, 2);
    assert_eq!(next.len(), 2);
    assert_eq!(next[0].1, Phase::Dark);
    near(next[0].0, at(2026, 6, 21, 19, 33), "закат");
    assert_eq!(next[1].1, Phase::Light);
    near(next[1].0, at(2026, 6, 22, 2, 43), "восход назавтра");
    // Сразу после заката следующая — восход, а не тот же закат.
    let after = next_sun_switches(next[0].0, lat, lon, 1);
    assert_eq!(after[0].1, Phase::Light);
}

fn full_config(mode: Mode, schedule: Schedule) -> Config {
    let pair = |l: &str, d: &str| Pair { on: true, light: l.into(), dark: d.into() };
    Config {
        mode,
        schedule,
        light_at: "07:00".into(),
        dark_at: "19:30".into(),
        auto_location: false,
        lat: Some(52.52),
        lon: Some(13.405),
        global: pair("org.kde.breeze.desktop", "org.kde.breezedark.desktop"),
        colors: pair("BreezeLight", "BreezeDark"),
        gtk: pair("Adwaita", "Adwaita-dark"),
        icons: pair("Papirus-Light", "Papirus-Dark"),
        wallpaper: pair("/home/u/day.png", "/home/u/night.png"),
        portal: true,
    }
}

const DES: &[&str] = &["kde", "gnome", "cinnamon", "mate", "xfce", "budgie", "pantheon", "lxqt", "cosmic", "hyprland", "sway", "deepin"];

fn cases() -> Value {
    let mut plans = serde_json::Map::new();
    for de in DES {
        let auto = full_config(Mode::Auto, Schedule::Sun);
        let mut row = serde_json::Map::new();
        for (name, phase) in [("light", Phase::Light), ("dark", Phase::Dark)] {
            row.insert(format!("{name}_scheduled"), json!(plan(&auto, de, phase, true)));
        }
        row.insert("dark_fixed".into(), json!(plan(&full_config(Mode::Dark, Schedule::Sun), de, Phase::Dark, true)));
        row.insert("needs_timer".into(), json!(needs_timer(&auto, de)));
        row.insert("needs_timer_global_only".into(), json!(needs_timer(&Config { colors: Pair::default(), gtk: Pair::default(), icons: Pair::default(), wallpaper: Pair::default(), ..auto.clone() }, de)));
        plans.insert(de.to_string(), Value::Object(row));
    }
    let times = full_config(Mode::Auto, Schedule::Times);
    let sun = full_config(Mode::Auto, Schedule::Sun);
    let events = next_sun_switches(at(2026, 11, 1, 12, 0), 52.52, 13.405, 2).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
    json!({
        "plans": plans,
        "timer_times": timer_text(&times, &[]),
        "timer_sun": timer_text(&sun, &events),
        "service": service_text("/usr/lib/rexon/rexon-main-window --apply-day-night", Some("/usr/lib/rexon/rexon-main-window")),
        "autostart": autostart_text("/usr/lib/rexon/rexon-main-window --apply-day-night --login"),
        "kde_times": kde_native_argv(&times, false),
        "kde_sun_manual": kde_native_argv(&sun, false),
        "kde_sun_auto_location": kde_native_argv(&Config { auto_location: true, ..sun.clone() }, false),
        "kde_dark_fixed_was_auto": kde_native_argv(&full_config(Mode::Dark, Schedule::Sun), true),
        "kde_off_was_auto": kde_native_argv(&Config::default(), true),
    })
}

#[test]
fn эталон_планов_и_файлов() {
    let got = cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("эталона нет — снять: REXON_GOLDEN_WRITE=1")).unwrap();
    assert_eq!(got, want, "эталон {GOLDEN} разошёлся");
}

// ── Поддельная машина ───────────────────────────────────────────────────

#[derive(Default)]
struct Fake {
    files: RefCell<BTreeMap<String, String>>,
    runs: RefCell<Vec<Vec<String>>>,
    applied: RefCell<Vec<String>>,
}

impl DayNightIo for Fake {
    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        self.files.borrow_mut().insert(path.to_string(), text.to_string());
        true
    }
    fn remove(&self, path: &str) -> bool {
        self.files.borrow_mut().remove(path);
        true
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let prefix = format!("{path}/");
        let mut out: Vec<String> = self
            .files
            .borrow()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|r| r.split('/').next().unwrap().to_string()))
            .collect();
        out.dedup();
        out
    }
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.runs.borrow_mut().push(argv.to_vec());
        (true, String::new())
    }
    fn have(&self, _tool: &str) -> bool {
        true
    }
    fn apply_theme(&self, de: &str, kind: &str, name: &str) -> (bool, String) {
        self.applied.borrow_mut().push(format!("{de}:{kind}:{name}"));
        (true, String::new())
    }
    fn apply_wallpaper(&self, path: &str) -> (bool, String) {
        self.applied.borrow_mut().push(format!("wallpaper:{path}"));
        (true, String::new())
    }
}

fn env<'a>(io: &'a Fake, de: &str, now: i64) -> Env<'a> {
    Env {
        io,
        paths: Paths { config_home: "/h/.config".into(), state_home: "/h/.local/state".into() },
        clock: Clock { now, offset: 3600 },
        de: Some(de.to_string()),
        tz: (None, Some("/usr/share/zoneinfo/Europe/Berlin".into())),
        launch: Some(("/r/rexon --apply-day-night".into(), "/r/rexon --apply-day-night --login".into(), Some("/r/rexon".into()))),
    }
}

fn systemctl(io: &Fake) -> Vec<String> {
    io.runs.borrow().iter().filter(|a| a[0] == "systemctl").map(|a| a[2..].join(" ")).collect()
}

#[test]
fn plasma_только_глобальная_тема_отдаётся_plasma() {
    let io = Fake::default();
    let cfg = Config {
        colors: Pair::default(),
        gtk: Pair::default(),
        icons: Pair::default(),
        wallpaper: Pair::default(),
        ..full_config(Mode::Auto, Schedule::Times)
    };
    let (ok, msg) = save(&env(&io, "kde", at(2026, 11, 1, 12, 0)), &cfg);
    assert!(ok, "{msg}");
    // Ни таймера, ни своего применения: всё делает Plasma.
    assert!(systemctl(&io).is_empty(), "{:?}", io.runs.borrow());
    assert!(io.applied.borrow().is_empty(), "{:?}", io.applied.borrow());
    let runs = io.runs.borrow();
    let last = runs.last().unwrap();
    assert_eq!(last[..], ["kwriteconfig6", "--file", "kdeglobals", "--group", "KDE", "--key", "AutomaticLookAndFeel", "--notify", "true"]);
    assert!(runs.iter().any(|a| a.contains(&"SunsetStart".to_string()) && a.last().unwrap() == "19:30:00"));
}

#[test]
fn gnome_по_времени_таймер_и_применение() {
    let io = Fake::default();
    let cfg = full_config(Mode::Auto, Schedule::Times);
    // 20:00 по местным (UTC+1) — тёмное.
    let (ok, msg) = save(&env(&io, "gnome", at(2026, 11, 1, 19, 0)), &cfg);
    assert!(ok, "{msg}");
    assert_eq!(systemctl(&io), ["daemon-reload", "enable rexon-day-night.timer", "restart rexon-day-night.timer"]);
    let files = io.files.borrow();
    let timer = &files["/h/.config/systemd/user/rexon-day-night.timer"];
    assert!(timer.contains("OnCalendar=*-*-* 07:00:00\nOnCalendar=*-*-* 19:30:00\n"), "{timer}");
    assert!(files.contains_key("/h/.config/autostart/rexon-day-night.desktop"));
    assert_eq!(
        *io.applied.borrow(),
        ["gnome:colors:BreezeDark", "gnome:icons:Papirus-Dark", "gnome:gtk:Adwaita-dark", "wallpaper:/home/u/night.png"]
    );
    assert!(io.runs.borrow().iter().any(|a| a.join(" ") == "gsettings set org.gnome.desktop.interface color-scheme prefer-dark"));
    let memo: Value = serde_json::from_str(&files["/h/.local/state/rexon/day-night.json"]).unwrap();
    assert_eq!(memo["de"], "gnome");
    assert_eq!(memo["phase"], "dark");
}

#[test]
fn таймер_без_окружения_в_переменных_берёт_окружение_входа_и_переписывается_по_солнцу() {
    let io = Fake::default();
    let cfg = full_config(Mode::Auto, Schedule::Sun);
    let (ok, msg) = save(&env(&io, "xfce", at(2026, 11, 1, 10, 0)), &cfg);
    assert!(ok, "{msg}");
    let before = io.files.borrow()["/h/.config/systemd/user/rexon-day-night.timer"].clone();
    io.runs.borrow_mut().clear();
    io.applied.borrow_mut().clear();
    // Служба systemd: XDG_CURRENT_DESKTOP нет, время — после заката.
    let mut e = env(&io, "", at(2026, 11, 1, 17, 0));
    e.de = None;
    assert_eq!(run_scheduled(&e, false, &mut |_| {}), 0);
    assert_eq!(io.applied.borrow()[0], "xfce:colors:BreezeDark");
    assert!(io.runs.borrow().iter().any(|a| a.join(" ") == "gsettings set org.x.apps.portal color-scheme prefer-dark"));
    let after = io.files.borrow()["/h/.config/systemd/user/rexon-day-night.timer"].clone();
    assert_ne!(before, after, "после заката таймер ждёт уже следующий восход");
    assert_eq!(systemctl(&io), ["daemon-reload", "enable rexon-day-night.timer", "restart rexon-day-night.timer"]);
}

#[test]
fn светлое_вручную_снимает_таймер() {
    let io = Fake::default();
    let e = env(&io, "cinnamon", at(2026, 11, 1, 12, 0));
    assert!(save(&e, &full_config(Mode::Auto, Schedule::Times)).0);
    io.runs.borrow_mut().clear();
    io.applied.borrow_mut().clear();
    let (ok, msg) = save(&e, &full_config(Mode::Light, Schedule::Times));
    assert!(ok, "{msg}");
    assert_eq!(systemctl(&io), ["disable --now rexon-day-night.timer", "daemon-reload"]);
    let files = io.files.borrow();
    assert!(!files.keys().any(|k| k.contains("rexon-day-night.")), "{:?}", files.keys());
    assert_eq!(io.applied.borrow()[0], "cinnamon:colors:BreezeLight");
}

#[test]
fn cosmic_по_солнцу_отдаётся_cosmic() {
    let io = Fake::default();
    let cfg = Config {
        colors: Pair::default(),
        gtk: Pair::default(),
        icons: Pair::default(),
        wallpaper: Pair::default(),
        ..full_config(Mode::Auto, Schedule::Sun)
    };
    assert!(save(&env(&io, "cosmic", at(2026, 11, 1, 12, 0)), &cfg).0);
    assert_eq!(io.files.borrow()["/h/.config/cosmic/com.system76.CosmicTheme.Mode/v1/auto_switch"], "true");
    assert!(systemctl(&io).is_empty());
    // По времени — COSMIC так не умеет: переключает Rexon.
    let io = Fake::default();
    let cfg = Config { schedule: Schedule::Times, ..cfg };
    assert!(save(&env(&io, "cosmic", at(2026, 11, 1, 12, 0)), &cfg).0);
    let files = io.files.borrow();
    assert_eq!(files["/h/.config/cosmic/com.system76.CosmicTheme.Mode/v1/auto_switch"], "false");
    assert_eq!(files["/h/.config/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"], "false");
    assert!(files.contains_key("/h/.config/systemd/user/rexon-day-night.timer"));
}

#[test]
fn проверка_настройки_и_deepin() {
    let io = Fake::default();
    let e = env(&io, "gnome", 0);
    let bad = Config { light_at: "7".into(), ..full_config(Mode::Auto, Schedule::Times) };
    assert!(!save(&e, &bad).0);
    let same = Config { dark_at: "07:00".into(), ..full_config(Mode::Auto, Schedule::Times) };
    assert!(!save(&e, &same).0);
    let half = Config { gtk: Pair { on: true, light: "Adwaita".into(), dark: String::new() }, ..full_config(Mode::Dark, Schedule::Sun) };
    assert!(!save(&e, &half).0);
    assert!(io.files.borrow().is_empty(), "негодная настройка ничего не пишет");
    let (ok, _) = save(&env(&io, "deepin", 0), &full_config(Mode::Dark, Schedule::Sun));
    assert!(!ok);
}

#[test]
fn пары_по_именам_и_списки() {
    let io = Fake::default();
    for (path, text) in [
        ("/usr/share/themes/Adwaita/gtk-3.0/gtk.css", ""),
        ("/usr/share/themes/Adwaita-dark/gtk-3.0/gtk.css", ""),
        ("/usr/share/themes/Emacs/gtk-2.0/gtkrc", ""),
        ("/usr/share/color-schemes/BreezeLight.colors", "[General]\nName=Breeze Light\n"),
        ("/usr/share/color-schemes/BreezeDark.colors", "[General]\nName=Breeze Dark\n"),
        ("/usr/share/icons/breeze/index.theme", "[Icon Theme]\nName=Breeze\nDirectories=actions/16\n"),
        ("/usr/share/icons/breeze-dark/index.theme", "[Icon Theme]\nName=Breeze Dark\nDirectories=actions/16\n"),
        ("/usr/share/icons/Bibata/index.theme", "[Icon Theme]\nName=Bibata\n"),
        ("/usr/share/plasma/look-and-feel/org.kde.breeze.desktop/metadata.json", r#"{"KPlugin":{"Name":"Breeze"}}"#),
        ("/usr/share/plasma/look-and-feel/org.kde.breezedark.desktop/metadata.json", r#"{"KPlugin":{"Name":"Breeze Dark"}}"#),
    ] {
        io.write(path, text);
    }
    let ids = |kind: &str| list_items(&io, "/h", kind).into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids("gtk"), ["Adwaita", "Adwaita-dark"]);
    assert_eq!(ids("icons"), ["breeze", "breeze-dark"], "тема курсоров без каталогов значков — не сюда");
    for (kind, light, dark) in [
        ("gtk", "Adwaita", "Adwaita-dark"),
        ("colors", "BreezeLight", "BreezeDark"),
        ("icons", "breeze", "breeze-dark"),
        ("global", "org.kde.breeze.desktop", "org.kde.breezedark.desktop"),
    ] {
        assert_eq!(suggest_pair(&list_items(&io, "/h", kind)), Some((light.to_string(), dark.to_string())), "{kind}");
    }
}
