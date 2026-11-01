//! «Настроить…» у рамки окна KDE (`kdecoration::config_*`) — как кнопка
//! правки плитки окна «Оформление окон» Plasma 6.8. Ожидания —
//! `tests/golden/kdecoration_config_cases.json`.
//!
//! Последний тест открывает настоящее окно настроек Breeze на стенде, если на
//! машине есть запускатель `qml` Qt6, модуль превью KWin, Xvfb и
//! dbus-run-session; иначе печатает «НЕ ПРОВЕРЕНО».

use rexon_core::kdecoration as kd;
use serde_json::Value;

mod i18n_pin;

const SVG_LIBRARY: &str = "org.kde.kwin.aurorae.v2";

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/kdecoration_config_cases.json")).expect("эталон читается")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

fn target_of(g: &Value, c: &Value) -> Option<kd::ConfigTarget> {
    let files = strings(&g["files"]);
    let metas = g["plugin_meta"].clone();
    kd::config_target(
        c["id"].as_str().unwrap(),
        c["path"].as_str().unwrap(),
        c["caption"].as_str().unwrap(),
        SVG_LIBRARY,
        &|p| files.iter().any(|f| f == p),
        &|p| metas.get(p).cloned(),
    )
}

#[test]
fn target_finds_settings_module_like_kde() {
    let g = golden();
    for c in g["targets"].as_array().unwrap() {
        let got = target_of(&g, c);
        let want = (!c["want"].is_null()).then(|| kd::ConfigTarget {
            library: c["want"]["library"].as_str().unwrap().into(),
            theme: c["want"]["theme"].as_str().unwrap().into(),
            engine_theme: c["want"]["engine_theme"].as_bool().unwrap(),
            kcmodule: c["want"]["kcmodule"].as_str().unwrap().into(),
            caption: c["caption"].as_str().unwrap().into(),
        });
        assert_eq!(got, want, "{}", c["name"]);
    }
}

#[test]
fn kcm_plugin_is_looked_up_in_both_namespaces() {
    let g = golden();
    let files = strings(&g["files"]);
    for c in g["kcm_plugin"].as_array().unwrap() {
        let got = kd::kcm_plugin(c["kcm"].as_str().unwrap(), &strings(&c["dirs"]), &|p| files.iter().any(|f| f == p));
        assert_eq!(got.as_deref(), c["want"].as_str(), "{}", c["name"]);
    }
}

#[test]
fn button_opens_what_kde_opens_or_says_why_not() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let targets = g["targets"].as_array().unwrap();
    let pick = |key: &str| {
        let idx = match key {
            "breeze" => 0,
            "plain" => 1,
            "svg" => 2,
            other => panic!("нет цели {other}"),
        };
        target_of(&g, &targets[idx])
    };
    for c in g["buttons"].as_array().unwrap() {
        let target = c["target"].as_str().and_then(pick);
        let f = &c["facts"];
        let facts = kd::ConfigFacts {
            plasma: f["plasma"].as_bool().unwrap(),
            runner: f["runner"].as_str().map(String::from),
            kcm: f["kcm"].as_str().map(String::from),
            kcmshell: f["kcmshell"].as_bool().unwrap(),
        };
        let b = kd::config_button(target.as_ref(), &facts, "/c/kdecoration-configure.qml");
        let name = c["name"].as_str().unwrap();
        assert_eq!(b.visible, c["visible"].as_bool().unwrap(), "{name}");
        assert_eq!(b.enabled, c["enabled"].as_bool().unwrap(), "{name}");
        assert_eq!(b.argv, strings(&c["argv"]), "{name}");
        if b.visible {
            assert_eq!(b.text, "Настроить…", "{name}");
            // Нажать нельзя — причина обязана быть сказана; можно — сказано,
            // что откроется.
            assert!(!b.tooltip.is_empty(), "{name}: нет подсказки");
            assert!(b.tooltip.chars().any(|ch| ('а'..='я').contains(&ch)), "{name}: подсказка не переведена: {}", b.tooltip);
        }
    }
}

#[test]
fn script_is_the_data_file_and_calls_kwin_configure() {
    assert!(kd::CONFIG_SCRIPT.contains("org.kde.kwin.private.kdecoration"));
    assert!(kd::CONFIG_SCRIPT.contains("kcmoduleName"));
    assert!(kd::CONFIG_SCRIPT.contains("bridge.configure("));
    // Аргументов после `--` ровно столько, сколько кладёт кнопка.
    assert!(kd::CONFIG_SCRIPT.contains("arguments.length - 4"));
    let dir = std::env::temp_dir().join(format!("rexon-kdeco-config-{}", std::process::id()));
    let script = dir.join(kd::CONFIG_SCRIPT_NAME).to_string_lossy().into_owned();
    let argv = vec!["qml".to_string(), "-a".into(), "widget".into(), script.clone()];
    assert!(kd::ensure_config_script(&argv));
    assert_eq!(std::fs::read_to_string(&script).unwrap(), kd::CONFIG_SCRIPT);
    // Кнопка без сценария (kcmshell6) ничего не пишет.
    assert!(kd::ensure_config_script(&["kcmshell6".to_string(), "kcm_kwindecoration".to_string()]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Живое окно настроек Breeze на своём Xvfb и своей шине: процесс живёт,
/// пока окно открыто, и сам завершается с кодом 0, когда его закрыли.
#[test]
fn live_breeze_dialog_opens_and_exits_on_close() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let is_file = |p: &str| std::path::Path::new(p).is_file();
    let path = std::env::var("PATH").ok();
    let have = |p: &str| rexon_core::utils::which(p, path.as_deref()).is_some();
    let breeze_kcm = kd::kcm_plugin("kcm_breezedecoration", &["/usr/lib/qt6/plugins".to_string()], &is_file);
    let Some(runner) = kd::shot_runner(&is_file).filter(|_| breeze_kcm.is_some()) else {
        println!("НЕ ПРОВЕРЕНО: нет запускателя qml Qt6, модуля превью KWin или модуля настроек Breeze");
        return;
    };
    if !(have("Xvfb") && have("dbus-run-session") && have("xdotool")) {
        println!("НЕ ПРОВЕРЕНО: нет Xvfb, dbus-run-session или xdotool");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rexon-kdeco-live-{}", std::process::id()));
    let home = dir.join("home");
    let rt = dir.join("rt");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&rt).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let script = dir.join(kd::CONFIG_SCRIPT_NAME).to_string_lossy().into_owned();
    let target = kd::ConfigTarget {
        library: "org.kde.breeze".into(),
        theme: "Breeze".into(),
        engine_theme: false,
        kcmodule: "kcm_breezedecoration".into(),
        caption: "RexonBreezeProbe".into(),
    };
    let facts = kd::ConfigFacts { plasma: true, runner: Some(runner), kcm: breeze_kcm, kcmshell: false };
    let b = kd::config_button(Some(&target), &facts, &script);
    assert!(b.enabled && kd::ensure_config_script(&b.argv));
    // Своё всё: дисплей, HOME, каталог сеанса, шина — живой сеанс не задет.
    let display = format!(":{}", 150 + std::process::id() % 40);
    let mut xvfb = Command::new("Xvfb")
        .args([display.as_str(), "-screen", "0", "1024x768x24", "-nolisten", "tcp"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let shell = format!(
        "{argv} >/dev/null 2>&1 & Q=$!; \
         W=; for i in $(seq 1 60); do \
           for w in $(xdotool search --onlyvisible --name '.*' 2>/dev/null); do \
             [ \"$(xdotool getwindowname $w 2>/dev/null)\" = RexonBreezeProbe ] && W=$w; \
           done; [ -n \"$W\" ] && break; sleep 0.25; done; \
         [ -z \"$W\" ] && {{ kill $Q; exit 11; }}; \
         kill -0 $Q || exit 12; \
         xdotool windowfocus $W; sleep 0.3; xdotool key Escape; \
         for i in $(seq 1 40); do kill -0 $Q 2>/dev/null || break; sleep 0.25; done; \
         kill -0 $Q 2>/dev/null && {{ kill $Q; exit 13; }}; wait $Q",
        argv = b.argv.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(" ")
    );
    let started = Instant::now();
    let status = Command::new("dbus-run-session")
        .args(["--", "sh", "-c", &shell])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &rt)
        .env("DISPLAY", &display)
        .env("QT_QPA_PLATFORM", "xcb")
        .stdin(Stdio::null())
        .status()
        .unwrap();
    let _ = xvfb.kill();
    let _ = xvfb.wait();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(started.elapsed() < Duration::from_secs(40));
    assert_eq!(status.code(), Some(0), "11 — окна нет, 12 — процесс умер сразу, 13 — не завершился после закрытия");
}
