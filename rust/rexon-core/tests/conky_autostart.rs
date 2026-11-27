//! Conky widget autostart ([`rexon_core::conky_autostart`]): golden
//! `.desktop` files (delay, «только здесь», overlay, KDE after the panel),
//! lines for compositors without XDG autostart, and a stand of a real headless sway
//! in a private home directory.

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
        rexon_core::ext_manager::conky_desktop_entry_with("My W", conf, None, None, 20, &["MATE".into()]),
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Conky — My W\n\
         Comment=Rexon Conky widget: My W\n\
         Exec=sh -c \"sleep 20 && x=; case \\\\\":\\\\$XDG_CURRENT_DESKTOP:\\\\\" in *:GNOME:*|*:X-Cinnamon:*|*:Cinnamon:*|*:Pantheon:*) x=1;; esac; case \\\\\"\\\\$(conky -v 2>/dev/null)\\\\\" in *\\\\\"conky 1.25.\\\\\"[012][!0-9]*) x=1;; esac; [ -n \\\\\"\\\\$x\\\\\" ] && [ -n \\\\\"\\\\$WAYLAND_DISPLAY\\\\\" ] && [ -n \\\\\"\\\\$DISPLAY\\\\\" ] && { unset WAYLAND_DISPLAY; export XDG_SESSION_TYPE=x11; }; cd \\\\\"\\\\$(dirname \\\\\"\\\\$0\\\\\")\\\\\" && exec conky -c \\\\\"\\\\${rexon_xw:-\\\\$0}\\\\\"\" \"/h/.config/conky/My W/conky.conf\"\n\
         Path=/h/.config/conky/My W\n\
         OnlyShowIn=MATE;\n\
         X-Rexon-Conky-Delay=20\n\
         X-GNOME-Autostart-enabled=true\n\
         X-KDE-autostart-after=panel\n\
         StartupNotify=false\n\
         Terminal=false\n"
    );
    // Without a delay and a desktop — the old form plus "after the panel".
    let plain = rexon_core::ext_manager::conky_desktop_entry("w", "/h/w/c.conf");
    assert!(!plain.contains("sleep") && !plain.contains("OnlyShowIn") && !plain.contains(ca::DELAY_KEY), "{plain}");
    assert!(plain.contains("\nX-KDE-autostart-after=panel\n"));
    // With an overlay: conky with the overlay, the delay is the same.
    let via = rexon_core::ext_manager::conky_desktop_entry_with("w", "/h/w/c.conf", Some("/s/o.rexon.conf"), None, 5, &[]);
    let exec = via.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
    let argv = rexon_core::compositor_autostart::exec_argv(exec).unwrap();
    // Before launch — switch to XWayland, where Wayland output is unsuitable.
    let script = format!(
        "sleep 5 && {}cd \"$(dirname \"$1\")\" && exec conky -c \"${{rexon_xw:-$0}}\"",
        rexon_core::conky_overlay::xwayland_shell(false, None)
    );
    assert_eq!(argv, ["sh", "-c", script.as_str(), "/s/o.rexon.conf", "/h/w/c.conf"]);
}

/// The delay from the entry through a real `sh`: conky starts no earlier than after
/// N seconds, from the widget's folder.
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
    let entry = rexon_core::ext_manager::conky_desktop_entry_with("w", &conf, None, None, 1, &[]);
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

/// The autostart entry through a real `sh`: in a Cinnamon Wayland session conky goes
/// through XWayland with the file where the window is below windows (if it exists); in KDE and without
/// the file — with the file from the entry.
#[test]
fn autostart_takes_xwayland_file() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("xwfile");
    let bin = d.join("bin");
    let w = d.join("it's w");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&w).unwrap();
    std::fs::write(
        bin.join("conky"),
        "#!/bin/sh\n[ \"$1\" = -v ] && { echo 'conky 1.26.0 compiled'; exit 0; }\necho \"$2 $XDG_SESSION_TYPE\"\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("conky"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let conf = w.join("conky.conf").to_string_lossy().into_owned();
    let xw = d.join("state/it's+x.rexon.conf").to_string_lossy().into_owned();
    let overlay = d.join("state/o.rexon.conf").to_string_lossy().into_owned();
    let run = |launch: Option<&str>, desk: &str| {
        let entry = rexon_core::ext_manager::conky_desktop_entry_with("w", &conf, launch, Some(&xw), 0, &[]);
        let exec = entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
        let argv = rexon_core::compositor_autostart::exec_argv(exec).unwrap();
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("XDG_CURRENT_DESKTOP", desk)
            .env("WAYLAND_DISPLAY", "wayland-0")
            .env("DISPLAY", ":0")
            .env("XDG_SESSION_TYPE", "wayland")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    // No XWayland file yet — conky with the author's file.
    let before = run(None, "X-Cinnamon");
    std::fs::create_dir_all(d.join("state")).unwrap();
    std::fs::write(&xw, "-- x").unwrap();
    let got = [before, run(None, "X-Cinnamon"), run(Some(&overlay), "X-Cinnamon"), run(None, "KDE")];
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(got, [format!("{conf} x11"), format!("{xw} x11"), format!("{xw} x11"), format!("{conf} wayland")]);
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
            // The system sway config: our own file is created only with its `include`.
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

/// A sway session: the launch line is a managed block in `~/.config/sway/config`
/// (the file is created), the entry is hidden from sway (`NotShowIn=sway`) so that
/// the widget does not start twice; removal restores everything as it was.
#[test]
fn sway_gets_its_own_line() {
    let d = tmp("sway");
    let env = sway_env(&d);
    let opts = Options { delay: 3, only_in: Vec::new() };
    let built = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, None, &opts);
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

    // «Только в MATE» in a sway session — sway has no line.
    let only = Options { delay: 3, only_in: vec!["MATE".into()] };
    let b2 = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, None, &only);
    assert!(!cfg.exists(), "заведённый файл снят вместе с блоком");
    assert!(!b2.desktop.contains("NotShowIn") && b2.desktop.contains("\nOnlyShowIn=MATE;\n"));
    // «Только в sway» — the line only in sway.
    let here = Options { delay: 0, only_in: ca::current_names(&env) };
    let b3 = ca::build(&env, "Jhalak", "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf", None, None, &here);
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("rexon:conky-Jhalak"));
    assert!(b3.desktop.contains("\nOnlyShowIn=sway;\n"));
    ca::remove(&env, "Jhalak");
    assert!(!cfg.exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// A path sway cannot hold (`$`) — an honest refusal with an explanation, not
/// a line that sway will expand its own way.
#[test]
fn unsafe_path_is_refused_in_compositor_session() {
    let d = tmp("refuse");
    let env = sway_env(&d);
    rexon_core::i18n::set_language(Some("en"));
    let built = ca::build(&env, "w", "w", "/h/$w/conky.conf", None, None, &Options::default());
    assert!(built.refusal.as_deref().is_some_and(|r| r.contains("Sway")), "{:?}", built.refusal);
    assert!(!d.join(".config/sway/config").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// The switch on real files: a new entry gets the default delay;
/// the settings change and survive rewriting an old-form entry.
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
    // An old-form entry with delay keys is rewritten without losing them.
    let old = format!(
        "[Desktop Entry]\nType=Application\nExec=conky -c '{home}/.config/conky/W/conky.conf'\nOnlyShowIn=MATE;\n{}=42\n",
        ca::DELAY_KEY
    );
    std::fs::write(&path, old).unwrap();
    assert!(da::conky_autostart(&SystemIo, &ctx, "W"));
    let now = std::fs::read_to_string(&path).unwrap();
    assert_eq!(ca::options_of(&now), Options { delay: 42, only_in: vec!["MATE".into()] });
    assert!(now.contains("X-KDE-autostart-after=panel") && now.contains("sleep 42 && "), "{now}");
    // Turning it off removes the entry.
    assert!(da::set_conky_autostart(&SystemIo, &ctx, "W", false).0);
    assert!(!std::path::Path::new(&path).exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Stand: a real headless sway (its own HOME and XDG_RUNTIME_DIR) reads
/// a config with the Rexon line and launches "conky" (a fake script records
/// where it was called from and with what). The person's live session is not involved.
/// No sway — the check is skipped.
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
    // Our own minimal config (without the system one: the stand needs no panel or
    // keys) — Rexon appends its block to it.
    std::fs::create_dir_all(home.join(".config/sway")).unwrap();
    std::fs::write(home.join(".config/sway/config"), "output * bg #000000 solid_color\n").unwrap();
    let env = sway_env(&home);
    let built = ca::build(&env, "W", "W", &conf, None, None, &Options { delay: 1, only_in: Vec::new() });
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

/// Wehrwolfmann's decision of 23.10.2026: the line is written only to the config of
/// the CURRENT session's compositor. Turned on in KDE — the sway and Hyprland configs are
/// untouched; turned on in sway — Hyprland is untouched; turning off removes the line
/// from everywhere it remained from earlier enables.
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

    // KDE: XDG autostart works by itself — no config is touched.
    let mut kde = env.clone();
    kde.current_desktop = vec!["KDE".into()];
    kde.vars.clear();
    let b = ca::build(&kde, "W", "W", conf, None, None, &opts);
    assert!(b.configs.is_empty() && b.refusal.is_none());
    assert!(!b.desktop.contains("NotShowIn"), "{}", b.desktop);
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // sway: the line only in sway, NotShowIn — only sway.
    let b = ca::build(&env, "W", "W", conf, None, None, &opts);
    assert_eq!(b.configs, [sway_cfg.to_string_lossy().into_owned()]);
    assert!(b.desktop.contains("\nNotShowIn=sway;\n"), "{}", b.desktop);
    assert!(std::fs::read_to_string(&sway_cfg).unwrap().contains("rexon:conky-W"));
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // Back in KDE (rewriting the entry): the sway line is refreshed and
    // stays hidden from sway, Hyprland is still untouched.
    let b = ca::build(&kde, "W", "W", conf, None, None, &Options { delay: 7, only_in: Vec::new() });
    assert!(b.desktop.contains("\nNotShowIn=sway;\n"), "{}", b.desktop);
    assert!(std::fs::read_to_string(&sway_cfg).unwrap().contains("sleep 7 && "));
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");

    // A leftover of a previous enable in Hyprland; turning off removes it everywhere.
    let mut hypr = env.clone();
    hypr.current_desktop = vec!["Hyprland".into()];
    hypr.vars.clear();
    hypr.vars.insert("HYPRLAND_INSTANCE_SIGNATURE".into(), "x".into());
    let b = ca::build(&hypr, "W", "W", conf, None, None, &opts);
    assert!(std::fs::read_to_string(&hypr_cfg).unwrap().contains("rexon:conky-W"));
    assert!(b.desktop.contains("\nNotShowIn=sway;Hyprland;\n"), "{}", b.desktop);
    // «Только в Hyprland» removes the sway line.
    let here = Options { delay: 0, only_in: ca::current_names(&hypr) };
    ca::build(&hypr, "W", "W", conf, None, None, &here);
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert!(std::fs::read_to_string(&hypr_cfg).unwrap().contains("rexon:conky-W"));
    ca::build(&env, "W", "W", conf, None, None, &opts);
    ca::remove(&kde, "W");
    assert_eq!(std::fs::read_to_string(&sway_cfg).unwrap(), "# мой sway\n");
    assert_eq!(std::fs::read_to_string(&hypr_cfg).unwrap(), "# мой Hyprland\n");
    let _ = std::fs::remove_dir_all(&d);
}
