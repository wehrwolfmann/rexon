//! Автозапуск виджета Conky ([`rexon_core::conky_autostart`]): эталоны
//! `.desktop` (задержка, «только здесь», накладка, KDE после панели),
//! строки композиторов без XDG-автозапуска и стенд настоящего sway без
//! экрана в частном домашнем каталоге.

use rexon_core::compositor_autostart::{Compositor, SessionEnv};
use rexon_core::conky_autostart::{self as ca, Options};

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rexon-conky-auto-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn golden_entries() {
    let conf = "/h/.config/conky/My W/conky.conf";
    assert_eq!(
        rexon_core::ext_manager::conky_desktop_entry_with("My W", conf, None, 20, &["MATE".into()]),
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Conky — My W\n\
         Comment=Rexon Conky widget: My W\n\
         Exec=sh -c \"sleep 20 && cd \\\\\"\\\\$(dirname \\\\\"\\\\$0\\\\\")\\\\\" && exec conky -c \\\\\"\\\\$0\\\\\"\" \"/h/.config/conky/My W/conky.conf\"\n\
         Path=/h/.config/conky/My W\n\
         OnlyShowIn=MATE;\n\
         X-Rexon-Conky-Delay=20\n\
         X-GNOME-Autostart-enabled=true\n\
         X-KDE-autostart-after=panel\n\
         StartupNotify=false\n\
         Terminal=false\n"
    );
    // Без задержки и окружения — прежний вид плюс «после панели».
    let plain = rexon_core::ext_manager::conky_desktop_entry("w", "/h/w/c.conf");
    assert!(!plain.contains("sleep") && !plain.contains("OnlyShowIn") && !plain.contains(ca::DELAY_KEY), "{plain}");
    assert!(plain.contains("\nX-KDE-autostart-after=panel\n"));
    // С накладкой: conky — с накладкой, задержка — та же.
    let via = rexon_core::ext_manager::conky_desktop_entry_with("w", "/h/w/c.conf", Some("/s/o.rexon.conf"), 5, &[]);
    let exec = via.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
    let argv = rexon_core::compositor_autostart::exec_argv(exec).unwrap();
    assert_eq!(
        argv,
        ["sh", "-c", "sleep 5 && cd \"$(dirname \"$1\")\" && exec conky -c \"$0\"", "/s/o.rexon.conf", "/h/w/c.conf"]
    );
}

/// Задержка из записи настоящим `sh`: conky запускается не раньше, чем через
/// N секунд, из папки виджета.
#[test]
fn delay_really_waits() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("delay");
    let bin = d.join("bin");
    let w = d.join("w x");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&w).unwrap();
    let log = d.join("log");
    std::fs::write(bin.join("conky"), format!("#!/bin/sh\npwd > '{}'\n", log.display())).unwrap();
    std::fs::set_permissions(bin.join("conky"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let conf = w.join("conky.conf").to_string_lossy().into_owned();
    let entry = rexon_core::ext_manager::conky_desktop_entry_with("w", &conf, None, 1, &[]);
    let exec = entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
    let argv = rexon_core::compositor_autostart::exec_argv(exec).unwrap();
    let t = std::time::Instant::now();
    let st = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir("/")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .status()
        .unwrap();
    assert!(st.success());
    assert!(t.elapsed() >= std::time::Duration::from_millis(950), "{:?}", t.elapsed());
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), w.to_string_lossy());
    let _ = std::fs::remove_dir_all(&d);
}

fn sway_env(home: &std::path::Path) -> SessionEnv {
    let h = home.to_string_lossy().into_owned();
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("SWAYSOCK".to_string(), "/run/fake.sock".to_string());
    SessionEnv {
        config_home: format!("{h}/.config"),
        home: h,
        config_dirs: vec!["/etc/xdg".into()],
        sysconf: {
            // Системный конфиг sway: свой файл заводится только с его `include`.
            let etc = home.join("etc");
            std::fs::create_dir_all(etc.join("sway")).unwrap();
            std::fs::write(etc.join("sway/config"), "# system\n").unwrap();
            etc.to_string_lossy().into_owned()
        },
        current_desktop: vec!["sway".into()],
        vars,
        installed: vec![Compositor::Sway],
    }
}

/// Сеанс sway: строка запуска — управляемым блоком в `~/.config/sway/config`
/// (файл заводится), запись скрыта от sway (`NotShowIn=sway`), чтобы
/// виджет не стартовал дважды; снятие возвращает всё, как было.
#[test]
fn sway_gets_its_own_line() {
    let d = tmp("sway");
    let env = sway_env(&d);
    let opts = Options { delay: 3, only_in: Vec::new() };
    let built = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, &opts);
    assert!(built.refusal.is_none(), "{:?}", built.refusal);
    assert!(built.desktop.contains("\nNotShowIn=sway;\n"), "{}", built.desktop);
    let cfg = d.join(".config/sway/config");
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        text.contains("exec sh -c 'sleep 3 && cd \"/h/.config/conky/Jhalak\" && exec conky -c \"/h/.config/conky/Jhalak/Jhalak.conf\"'"),
        "{text}"
    );
    assert!(text.contains("rexon:conky-Jhalak"), "{text}");
    assert_eq!(built.configs, [cfg.to_string_lossy().into_owned()]);

    // «Только в MATE» в сеансе sway — строки у sway нет.
    let only = Options { delay: 3, only_in: vec!["MATE".into()] };
    let b2 = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, &only);
    assert!(!cfg.exists(), "заведённый файл снят вместе с блоком");
    assert!(!b2.desktop.contains("NotShowIn") && b2.desktop.contains("\nOnlyShowIn=MATE;\n"));
    // «Только в sway» — строка только у sway.
    let here = Options { delay: 0, only_in: ca::current_names(&env) };
    let b3 = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, &here);
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("rexon:conky-Jhalak"));
    assert!(b3.desktop.contains("\nOnlyShowIn=sway;\n"));
    ca::remove(&env, "Jhalak");
    assert!(!cfg.exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Путь, который sway не удержит (`$`), — честный отказ с объяснением, а
/// не строка, которую sway развернёт по-своему.
#[test]
fn unsafe_path_is_refused_in_compositor_session() {
    let d = tmp("refuse");
    let env = sway_env(&d);
    rexon_core::i18n::set_language(Some("en"));
    let built = ca::build(&env, "w", "w", "/h/$w/conky.conf", None, &Options::default());
    assert!(built.refusal.as_deref().is_some_and(|r| r.contains("Sway")), "{:?}", built.refusal);
    assert!(!d.join(".config/sway/config").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Переключатель на настоящих файлах: новая запись — с задержкой по
/// умолчанию; настройки меняются и переживают перезапись старого вида.
#[test]
fn toggle_and_options_keep_each_other() {
    use rexon_core::animations::SystemIo;
    use rexon_core::de_apply::{self as da, Ctx};
    let d = tmp("toggle");
    let home = d.to_string_lossy().into_owned();
    std::fs::create_dir_all(d.join(".config/conky/W")).unwrap();
    std::fs::write(d.join(".config/conky/W/conky.conf"), "conky.config = {}\nconky.text = [[x]]\n").unwrap();
    std::env::set_var("XDG_STATE_HOME", d.join("state"));
    let ctx = Ctx { home: home.clone(), xdg_config_home: None, wayland: false, de: "mate".into() };
    let path = da::conky_autostart_path(&ctx, "W");
    let (ok, msg) = da::set_conky_autostart(&SystemIo, &ctx, "W", true);
    assert!(ok, "{msg}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(ca::options_of(&text).delay, ca::DEFAULT_DELAY);

    let (ok, _) = da::set_conky_autostart_options(&SystemIo, &ctx, "W", &Options { delay: 42, only_in: vec!["MATE".into()] });
    assert!(ok);
    // Запись старого вида с ключами задержки переписывается без потери их.
    let old = format!(
        "[Desktop Entry]\nType=Application\nExec=conky -c '{home}/.config/conky/W/conky.conf'\nOnlyShowIn=MATE;\n{}=42\n",
        ca::DELAY_KEY
    );
    std::fs::write(&path, old).unwrap();
    assert!(da::conky_autostart(&SystemIo, &ctx, "W"));
    let now = std::fs::read_to_string(&path).unwrap();
    assert_eq!(ca::options_of(&now), Options { delay: 42, only_in: vec!["MATE".into()] });
    assert!(now.contains("X-KDE-autostart-after=panel") && now.contains("sleep 42 && "), "{now}");
    // Выключение снимает запись.
    assert!(da::set_conky_autostart(&SystemIo, &ctx, "W", false).0);
    assert!(!std::path::Path::new(&path).exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Стенд: настоящий sway без экрана (свой HOME и XDG_RUNTIME_DIR) читает
/// конфиг со строкой Rexon и запускает «conky» (подставной сценарий пишет,
/// откуда и с чем его позвали). Живой сеанс человека не задействован.
/// Нет sway — проверка пропускается.
#[test]
fn headless_sway_starts_the_widget() {
    use std::os::unix::fs::PermissionsExt;
    if std::process::Command::new("sway").arg("--version").output().is_err() {
        eprintln!("НЕ ПРОВЕРЕНО: sway не стоит");
        return;
    }
    let d = tmp("swaystand");
    let home = d.join("home");
    let run = d.join("run");
    let bin = d.join("bin");
    let w = home.join(".config/conky/W");
    for p in [&home, &run, &bin, &w] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let log = d.join("started");
    std::fs::write(bin.join("conky"), format!("#!/bin/sh\n{{ pwd; printf '%s\\n' \"$@\"; }} > '{}'\n", log.display())).unwrap();
    std::fs::set_permissions(bin.join("conky"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let conf = w.join("conky.conf").to_string_lossy().into_owned();
    std::fs::write(&conf, "conky.text = [[x]]\n").unwrap();
    // Свой минимальный конфиг (без системного: стенду не нужны панель и
    // клавиши) — Rexon дописывает в него свой блок.
    std::fs::create_dir_all(home.join(".config/sway")).unwrap();
    std::fs::write(home.join(".config/sway/config"), "output * bg #000000 solid_color\n").unwrap();
    let env = sway_env(&home);
    let built = ca::build(&env, "W", "W", &conf, None, &Options { delay: 1, only_in: Vec::new() });
    assert!(built.refusal.is_none(), "{:?}", built.refusal);

    let mut sway = std::process::Command::new("sway")
        .arg("-c")
        .arg(home.join(".config/sway/config"))
        .env_clear()
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &run)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("WLR_BACKENDS", "headless")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .env("WLR_RENDERER", "pixman")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("sway запускается");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !log.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let _ = sway.kill();
    let _ = sway.wait();
    let got = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(got, format!("{}\n-c\n{conf}\n", w.display()), "sway запустил строку Rexon");
}

/// Решение Wehrwolfmann 23.10.2026: строка пишется только в конфиг
/// композитора ТЕКУЩЕГО сеанса. Включил в KDE — конфиги sway и Hyprland не
/// тронуты; включил в sway — Hyprland не тронут; выключение снимает строку
/// отовсюду, где она осталась от прежних включений.
#[test]
fn only_the_current_compositor_config_is_touched() {
    let d = tmp("current");
    let mut env = sway_env(&d);
    env.installed = vec![Compositor::Sway, Compositor::Hyprland];
    let sway_cfg = d.join(".config/sway/config");
    let hypr_cfg = d.join(".config/hypr/hyprland.conf");
    std::fs::create_dir_all(sway_cfg.parent().unwrap()).unwrap();
    std::fs::create_dir_all(hypr_cfg.parent().unwrap()).unwrap();
    std::fs::write(&sway_cfg, "# мой sway\n").unwrap();
    std::fs::write(&hypr_cfg, "# мой Hyprland\n").unwrap();
    let conf = "/h/.config/conky/W/conky.conf";
    let opts = Options { delay: 0, only_in: Vec::new() };

    // KDE: XDG-автозапуск работает сам — ни один конфиг не тронут.
    let mut kde = env.clone();
    kde.current_desktop = vec!["KDE".into()];
    kde.vars.clear();
    let b = ca::build(&kde, "W", "W", conf, None, &opts);
    assert!(b.configs.is_empty() && b.refusal.is_none());
    assert!(!b.desktop.contains("NotShowIn"), "{}", b.desktop);
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // sway: строка только у sway, NotShowIn — только sway.
    let b = ca::build(&env, "W", "W", conf, None, &opts);
    assert_eq!(b.configs, [sway_cfg.to_string_lossy().into_owned()]);
    assert!(b.desktop.contains("\nNotShowIn=sway;\n"), "{}", b.desktop);
    assert!(std::fs::read_to_string(&sway_cfg).unwrap().contains("rexon:conky-W"));
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // Снова в KDE (перезапись записи): строка sway подновляется и
    // остаётся скрытой от sway, Hyprland по-прежнему не тронут.
    let b = ca::build(&kde, "W", "W", conf, None, &Options { delay: 7, only_in: Vec::new() });
    assert!(b.desktop.contains("\nNotShowIn=sway;\n"), "{}", b.desktop);
    assert!(std::fs::read_to_string(&sway_cfg).unwrap().contains("sleep 7 && "));
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // Хвост прошлого включения в Hyprland; выключение снимает отовсюду.
    let mut hypr = env.clone();
    hypr.current_desktop = vec!["Hyprland".into()];
    hypr.vars.clear();
    hypr.vars.insert("HYPRLAND_INSTANCE_SIGNATURE".into(), "x".into());
    let b = ca::build(&hypr, "W", "W", conf, None, &opts);
    assert!(std::fs::read_to_string(&hypr_cfg).unwrap().contains("rexon:conky-W"));
    assert!(b.desktop.contains("\nNotShowIn=sway;Hyprland;\n"), "{}", b.desktop);
    // «Только в Hyprland» снимает строку sway.
    let here = Options { delay: 0, only_in: ca::current_names(&hypr) };
    ca::build(&hypr, "W", "W", conf, None, &here);
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert!(std::fs::read_to_string(&hypr_cfg).unwrap().contains("rexon:conky-W"));
    ca::build(&env, "W", "W", conf, None, &opts);
    ca::remove(&kde, "W");
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");
    let _ = std::fs::remove_dir_all(&d);
}
