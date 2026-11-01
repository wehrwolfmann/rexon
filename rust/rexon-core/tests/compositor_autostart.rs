//! Сверки автозапуска через конфиги композиторов на подставных домашних
//! каталогах: вставка, повтор без изменений, снятие бит-в-бит, конфиги с
//! include/source, отсутствующий конфиг. Живых конфигов не касается: HOME
//! каждой сверки — свой каталог под `CARGO_TARGET_TMPDIR`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use rexon_core::compositor_autostart::{
    self as ca, Change, Compositor, Refusal, SessionEnv, Syntax, TextEdit,
};

mod i18n_pin;

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Свежий подставной дом.
fn sandbox(name: &str) -> SessionEnv {
    let root = format!("{}/compositor_autostart/{name}", env!("CARGO_TARGET_TMPDIR"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(format!("{root}/home/.config")).unwrap();
    fs::create_dir_all(format!("{root}/etc/xdg")).unwrap();
    SessionEnv {
        home: format!("{root}/home"),
        config_home: format!("{root}/home/.config"),
        config_dirs: vec![format!("{root}/etc/xdg")],
        sysconf: format!("{root}/etc"),
        ..Default::default()
    }
}

fn put(path: &str, text: &str) {
    fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap()
}

const TRAY: &[&str] = &["/opt/Rexon Apps/rexon-main-window", "--tray"];

/// Полный цикл на одном файле: вставка → повтор (без изменений) → снятие
/// бит-в-бит. Возвращает текст после вставки.
fn cycle(env: &SessionEnv, c: Compositor, path: &str, before: Option<&str>) -> String {
    let mut env = env.clone();
    env.installed = vec![c];
    let r = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    let o = r.outcomes.iter().find(|o| o.compositor == c).expect("композитор в отчёте");
    assert_eq!(o.path, path, "{c:?}: не тот файл");
    assert!(o.result.is_ok(), "{c:?}: {:?}", o.result);
    let after = read(path);
    assert_ne!(Some(after.as_str()), before, "{c:?}: блок не лёг");

    // Повтор — файл не меняется ни на байт.
    let r2 = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    let o2 = r2.outcomes.iter().find(|o| o.compositor == c).unwrap();
    assert_eq!(o2.result, Ok(Change::Unchanged), "{c:?}: повтор что-то менял");
    assert_eq!(read(path), after);

    // Снятие — байт-в-байт прежний файл (или файла снова нет).
    ca::sync_command(&env, "tray", None);
    match before {
        Some(b) => assert_eq!(read(path), b, "{c:?}: снятие не вернуло прежний файл"),
        None => assert!(!Path::new(path).exists(), "{c:?}: заведённый файл не удалён"),
    }
    after
}

#[test]
fn sway_existing_config_with_include_and_no_trailing_newline() {
    let env = sandbox("sway_existing");
    let path = format!("{}/sway/config", env.config_home);
    let before = "include /etc/sway/config.d/*\nset $mod Mod4\nbindsym $mod+Return exec foot";
    put(&path, before);
    let after = cycle(&env, Compositor::Sway, &path, Some(before));
    assert!(after.contains("\nexec '/opt/Rexon Apps/rexon-main-window' --tray\n"), "{after}");
    assert!(after.contains("# >>> rexon:tray +nl >>>"), "{after}");
}

#[test]
fn sway_missing_user_config_includes_system_one() {
    let env = sandbox("sway_missing");
    put(&format!("{}/sway/config", env.sysconf), "bindsym Mod4+Return exec foot\n");
    let path = format!("{}/sway/config", env.config_home);
    let after = cycle(&env, Compositor::Sway, &path, None);
    let sys = format!("{}/sway/config", env.sysconf);
    assert!(after.contains(&format!("include {sys}\n")), "{after}");
    assert!(after.contains(" +new "), "{after}");
}

#[test]
fn sway_i3_config_is_used_when_it_comes_first() {
    let env = sandbox("sway_i3");
    let path = format!("{}/.i3/config", env.home);
    put(&path, "bar {}\n");
    cycle(&env, Compositor::Sway, &path, Some("bar {}\n"));
}

#[test]
fn sway_without_any_config_refuses() {
    let mut env = sandbox("sway_none");
    env.installed = vec![Compositor::Sway];
    env.vars.insert("SWAYSOCK".into(), "/run/x".into());
    let r = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    assert_eq!(r.outcomes[0].result, Err(Refusal::ConfigMissing));
    assert!(!Path::new(&format!("{}/sway/config", env.config_home)).exists());
    let msg = r.current_refusal(&env).expect("текущий сеанс sway — отказ сказан");
    assert!(msg.contains("Sway") && msg.contains("exec '/opt/Rexon Apps/rexon-main-window' --tray"), "{msg}");
    assert!(r.not_show_in().is_empty(), "без строки запись от sway не прячем");
}

#[test]
fn hyprland_lua_preferred_over_conf() {
    let env = sandbox("hypr_lua");
    let lua = format!("{}/hypr/hyprland.lua", env.config_home);
    let conf = format!("{}/hypr/hyprland.conf", env.config_home);
    let before_lua = "hl.config({ general = { gaps_in = 5 } })\n";
    put(&lua, before_lua);
    put(&conf, "source = ~/.config/hypr/extra.conf\n");
    let after = cycle(&env, Compositor::Hyprland, &lua, Some(before_lua));
    assert!(
        after.contains(
            "hl.on(\"hyprland.start\", function() hl.exec_cmd(\"'/opt/Rexon Apps/rexon-main-window' --tray\") end)\n"
        ),
        "{after}"
    );
    assert_eq!(read(&conf), "source = ~/.config/hypr/extra.conf\n", "conf не тронут");
}

#[test]
fn hyprland_legacy_conf_with_source() {
    let env = sandbox("hypr_conf");
    let conf = format!("{}/hypr/hyprland.conf", env.config_home);
    let before = "source = ~/.config/hypr/a.conf\nexec-once = waybar\n";
    put(&conf, before);
    let after = cycle(&env, Compositor::Hyprland, &conf, Some(before));
    assert!(after.ends_with("exec-once = '/opt/Rexon Apps/rexon-main-window' --tray\n# <<< rexon:tray <<<\n"), "{after}");
}

#[test]
fn hyprland_missing_config_is_not_created() {
    let mut env = sandbox("hypr_none");
    env.installed = vec![Compositor::Hyprland];
    let r = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    assert_eq!(r.outcomes[0].result, Err(Refusal::ConfigMissing));
    assert!(!Path::new(&format!("{}/hypr", env.config_home)).exists());
}

#[test]
fn hyprland_conf_refuses_hash_in_command() {
    assert_eq!(
        ca::launch_line(Syntax::HyprConf, "t", &argv(&["/a#b/x"])),
        Err(Refusal::BadCharacters)
    );
    assert!(ca::launch_line(Syntax::HyprLua, "t", &argv(&["/a#b/x"])).is_ok());
}

/// sway делит строку `exec` на команды по `,` и `;` вне кавычек, а у
/// команды из ОДНОГО слова в кавычках снимает кавычки до `sh -c`. Сверено
/// на sway 1.12 без экрана: `exec touch /x/a,b` создавал `/x/a` и ругался
/// «Unknown/invalid command 'b'», `exec '/x/my dir/prog'` не запускал
/// ничего — оболочка получала `/x/my` и `dir/prog`.
#[test]
fn sway_line_survives_comma_and_single_spaced_word() {
    assert_eq!(
        ca::launch_line(Syntax::Sway, "t", &argv(&["touch", "/x/a,b"])),
        Ok(vec!["exec touch '/x/a,b'".to_string()])
    );
    assert_eq!(
        ca::launch_line(Syntax::Sway, "t", &argv(&["/x/my dir/prog"])),
        Ok(vec!["exec \"'/x/my dir/prog'\"".to_string()])
    );
    // Одно слово без кавычек и несколько слов — как прежде.
    assert_eq!(
        ca::launch_line(Syntax::Sway, "t", &argv(&["/usr/bin/rexon"])),
        Ok(vec!["exec /usr/bin/rexon".to_string()])
    );
    // Двойная кавычка внутри единственного слова обёртку бы разорвала.
    assert_eq!(
        ca::launch_line(Syntax::Sway, "t", &argv(&["/x/a \"b\""])),
        Err(Refusal::BadCharacters)
    );
}

#[test]
fn niri_kdl_and_niri_config_env() {
    let env = sandbox("niri");
    let path = format!("{}/niri/config.kdl", env.config_home);
    let before = "include \"extra.kdl\"\ninput {\n    keyboard { }\n}\n";
    put(&path, before);
    let after = cycle(&env, Compositor::Niri, &path, Some(before));
    assert!(after.contains("spawn-at-startup \"/opt/Rexon Apps/rexon-main-window\" \"--tray\"\n"), "{after}");
    assert!(after.contains("// >>> rexon:tray >>>"), "{after}");

    let mut env2 = sandbox("niri_env");
    let custom = format!("{}/my-niri.kdl", env2.home);
    put(&custom, "layout {}\n");
    env2.vars.insert("NIRI_CONFIG".into(), custom.clone());
    cycle(&env2, Compositor::Niri, &custom, Some("layout {}\n"));
}

#[test]
fn niri_missing_config_is_not_created() {
    let mut env = sandbox("niri_none");
    env.installed = vec![Compositor::Niri];
    let r = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    assert_eq!(r.outcomes[0].result, Err(Refusal::ConfigMissing));
}

#[test]
fn river_init_inserted_before_exec_and_stays_executable() {
    let env = sandbox("river");
    let path = format!("{}/river/init", env.config_home);
    let before = "#!/bin/sh\nriverctl map normal Super Return spawn foot\nexec rill\n";
    put(&path, before);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let mut e = env.clone();
    e.installed = vec![Compositor::River];
    ca::sync_command(&e, "tray", Some(&argv(TRAY)));
    let after = read(&path);
    let pos_block = after.find("'/opt/Rexon Apps/rexon-main-window' --tray &").expect("строка есть");
    assert!(pos_block < after.find("exec rill").unwrap(), "блок обязан стоять до exec: {after}");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
    ca::sync_command(&e, "tray", None);
    assert_eq!(read(&path), before);
}

#[test]
fn river_non_shell_init_refused() {
    let mut env = sandbox("river_bin");
    let path = format!("{}/river/init", env.config_home);
    put(&path, "#!/usr/bin/env python3\nprint(1)\n");
    env.installed = vec![Compositor::River];
    let r = ca::sync_command(&env, "tray", Some(&argv(TRAY)));
    assert_eq!(r.outcomes[0].result, Err(Refusal::Unsupported));
    assert_eq!(read(&path), "#!/usr/bin/env python3\nprint(1)\n");
}

#[test]
fn labwc_existing_and_created_with_system_autostart() {
    let env = sandbox("labwc");
    let path = format!("{}/labwc/autostart", env.config_home);
    let before = "swaybg -c '#113344' >/dev/null 2>&1 &\nwaybar &";
    put(&path, before);
    cycle(&env, Compositor::Labwc, &path, Some(before));

    let env2 = sandbox("labwc_new");
    let sys = format!("{}/labwc/autostart", env2.config_dirs[0]);
    put(&sys, "waybar &\n");
    let path2 = format!("{}/labwc/autostart", env2.config_home);
    let after = cycle(&env2, Compositor::Labwc, &path2, None);
    assert!(after.contains(&format!(". {sys}\n")), "системный autostart исполняется первым: {after}");
}

#[test]
fn wayfire_inside_existing_section_and_plugin_check() {
    let env = sandbox("wayfire");
    let path = format!("{}/wayfire.ini", env.config_home);
    let before = "[core]\nplugins = \\\n  autostart \\\n  command\n\n[autostart]\nautostart_wf_shell = true\n";
    put(&path, before);
    let after = cycle(&env, Compositor::Wayfire, &path, Some(before));
    let head = after.find("[autostart]\n").unwrap();
    let key = after.find("rexon_tray = '/opt/Rexon Apps/rexon-main-window' --tray").unwrap();
    assert!(head < key && key < after.find("autostart_wf_shell").unwrap(), "{after}");

    let mut off = sandbox("wayfire_off");
    let p = format!("{}/wayfire.ini", off.config_home);
    put(&p, "[core]\nplugins = command expo\n");
    off.installed = vec![Compositor::Wayfire];
    let r = ca::sync_command(&off, "tray", Some(&argv(TRAY)));
    assert_eq!(r.outcomes[0].result, Err(Refusal::PluginOff));
}

#[test]
fn wayfire_missing_config_created_and_vendored_path() {
    let env = sandbox("wayfire_new");
    let path = format!("{}/wayfire.ini", env.config_home);
    let after = cycle(&env, Compositor::Wayfire, &path, None);
    assert!(after.contains("[autostart]\nrexon_tray = "), "{after}");

    let env2 = sandbox("wayfire_vendored");
    let vendored = format!("{}/wayfire/wayfire.ini", env2.config_home);
    put(&vendored, "[core]\n");
    cycle(&env2, Compositor::Wayfire, &vendored, Some("[core]\n"));
}

#[test]
fn update_changes_only_body_and_keeps_flags() {
    let env = sandbox("update");
    let path = format!("{}/sway/config", env.config_home);
    put(&path, "bar {}");
    let mut e = env.clone();
    e.installed = vec![Compositor::Sway];
    ca::sync_command(&e, "tray", Some(&argv(&["/old/rexon", "--tray"])));
    ca::sync_command(&e, "tray", Some(&argv(&["/new/rexon", "--tray"])));
    let t = read(&path);
    assert!(t.contains("exec /new/rexon --tray") && !t.contains("/old/"), "{t}");
    ca::sync_command(&e, "tray", None);
    assert_eq!(read(&path), "bar {}");
}

#[test]
fn two_ids_live_side_by_side_and_come_off_independently() {
    let env = sandbox("two_ids");
    let path = format!("{}/niri/config.kdl", env.config_home);
    put(&path, "layout {}\n");
    let mut e = env.clone();
    e.installed = vec![Compositor::Niri];
    ca::sync_command(&e, "tray", Some(&argv(TRAY)));
    let with_tray = read(&path);
    ca::sync_command(&e, "session-splash", Some(&argv(&["/x/splash"])));
    ca::sync_command(&e, "session-splash", None);
    assert_eq!(read(&path), with_tray);
    ca::sync_command(&e, "tray", None);
    assert_eq!(read(&path), "layout {}\n");
}

#[test]
fn not_installed_compositor_gets_block_removed() {
    let env = sandbox("uninstalled");
    let path = format!("{}/sway/config", env.config_home);
    put(&path, "bar {}\n");
    let mut e = env.clone();
    e.installed = vec![Compositor::Sway];
    ca::sync_command(&e, "tray", Some(&argv(TRAY)));
    e.installed.clear();
    ca::sync_command(&e, "tray", Some(&argv(TRAY)));
    assert_eq!(read(&path), "bar {}\n");
}

#[test]
fn mirror_desktop_hides_entry_only_where_line_landed() {
    let mut env = sandbox("mirror");
    put(&format!("{}/sway/config", env.config_home), "bar {}\n");
    put(&format!("{}/niri/config.kdl", env.config_home), "layout {}\n");
    env.installed = vec![Compositor::Sway, Compositor::Niri, Compositor::Hyprland];
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Rexon\nExec={} --tray\n",
        rexon_core::installer::desktop_exec_arg(TRAY[0])
    );
    let m = ca::mirror_desktop(&env, "tray", &desktop);
    assert!(m.desktop.contains("NotShowIn=sway;niri;\n"), "{}", m.desktop);
    assert!(!m.desktop.contains("Hyprland"), "у Hyprland конфига нет — запись не прячем");
    assert!(read(&format!("{}/niri/config.kdl", env.config_home))
        .contains("spawn-at-startup \"/opt/Rexon Apps/rexon-main-window\" \"--tray\""));
}

#[test]
fn session_detection_prefers_full_desktop() {
    let mut env = sandbox("detect");
    env.vars.insert("LABWC_PID".into(), "1".into());
    env.current_desktop = vec!["Budgie".into(), "GNOME".into()];
    assert_eq!(ca::current_session(&env), ca::CurrentSession::FullDesktop);
    env.current_desktop = vec!["labwc".into(), "wlroots".into()];
    assert_eq!(ca::current_session(&env), ca::CurrentSession::Compositor(Compositor::Labwc));
    env.vars.clear();
    env.current_desktop = vec!["Hyprland".into()];
    assert_eq!(ca::current_session(&env), ca::CurrentSession::Compositor(Compositor::Hyprland));
    env.current_desktop.clear();
    assert_eq!(ca::current_session(&env), ca::CurrentSession::Unknown);
}

#[test]
fn status_is_truthful_in_compositor_session() {
    let mut env = sandbox("status");
    env.vars.insert("NIRI_SOCKET".into(), "/run/niri".into());
    env.installed = vec![Compositor::Niri];
    let desktop = "[Desktop Entry]\nType=Application\nExec=x\n";
    // Конфига нет, XDG-цель не поднята — не сработает.
    assert!(!ca::works_in_current_session(&env, "tray", Some(desktop), || false));
    // niri-session поднимает xdg-desktop-autostart.target — сработает.
    assert!(ca::works_in_current_session(&env, "tray", Some(desktop), || true));
    // Запись скрыта от niri, а строки нет — не сработает даже с целью.
    let hidden = ca::with_not_show_in(desktop, &["niri"]);
    assert!(!ca::works_in_current_session(&env, "tray", Some(&hidden), || true));
    // Строка на месте — сработает.
    put(&format!("{}/niri/config.kdl", env.config_home), "layout {}\n");
    ca::sync_command(&env, "tray", Some(&argv(&["x"])));
    assert!(ca::works_in_current_session(&env, "tray", Some(&hidden), || false));
    // Полное окружение — как прежде.
    env.vars.clear();
    env.current_desktop = vec!["KDE".into()];
    assert!(ca::works_in_current_session(&env, "nothing", None, || false));
}

#[test]
fn foreign_block_helper_roundtrip() {
    // Общий помощник для чужих строк (оболочка рабочего стола и т.п.).
    let env = sandbox("helper");
    let path = format!("{}/hypr/hyprland.conf", env.config_home);
    let before = "monitor=,preferred,auto,1\n";
    put(&path, before);
    let body = vec!["exec-once = qs -c noctalia-shell".to_string()];
    assert_eq!(ca::ensure_block(&path, Syntax::HyprConf, "shell", &body, None), Ok(Change::Written));
    assert_eq!(ca::ensure_block(&path, Syntax::HyprConf, "shell", &body, None), Ok(Change::Unchanged));
    assert_eq!(ca::remove_block(&path, Syntax::HyprConf, "shell"), Ok(Change::Removed));
    assert_eq!(read(&path), before);
    let missing = format!("{}/nope.conf", env.config_home);
    assert_eq!(ca::ensure_block(&missing, Syntax::HyprConf, "shell", &body, None), Err(Refusal::ConfigMissing));
    assert_eq!(ca::remove_block(&missing, Syntax::HyprConf, "shell"), Ok(Change::Unchanged));
}

/// Строки отказа есть в каталоге и переведены на русский и немецкий
/// (русский — источник смысла). Каталог собирается настоящим `msgfmt`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po настоящим msgfmt")]
fn refusal_texts_are_translated() {
    use rexon_core::i18n;
    let msgids = [
        "its settings file wasn't found",
        "its settings file is arranged in a way Rexon can't safely add to",
        "the launch command has characters this settings file can't hold",
        "startup programs are turned off in its settings",
        "its settings file couldn't be changed (%1)",
        "%1 doesn't start programs from the common startup list, and Rexon couldn't add itself to its settings: %2.",
        "Add this line to %1 yourself: %2",
    ];
    // Ключи каталога — ровно те строки, что отдаёт модуль. Язык закреплён:
    // иначе `text()` отдаёт перевод по локали машины (на русской или
    // немецкой сверка одна краснела), а в общем прогоне зеленела лишь
    // потому, что соседняя сверка успевала переключить язык процесса.
    let _lang = i18n_pin::en();
    assert_eq!(Refusal::ConfigMissing.text(), msgids[0]);
    assert_eq!(Refusal::Io("x".into()).text(), "its settings file couldn't be changed (x)");
    // Русский оригинал сверяется через собранный каталог: msgmerge переносит
    // длинный msgstr на несколько строк po, и поиск по тексту файла ломается.
    let ru_not_covered = "%1 не запускает программы из общего списка автозапуска, а вписать Rexon в его настройки не удалось: %2.";
    for lang in ["ru", "de"] {
        let po = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../po/{lang}.po"));
        let out = std::env::temp_dir().join(format!("rexon-ca-{lang}-{}.mo", std::process::id()));
        let ok = std::process::Command::new("msgfmt")
            .arg("--check")
            .arg("-o")
            .arg(&out)
            .arg(&po)
            .status()
            .expect("msgfmt запускается")
            .success();
        assert!(ok, "msgfmt отверг {}", po.display());
        let cat = i18n::Catalog::load(&out).expect("каталог читается");
        let _ = fs::remove_file(&out);
        for id in msgids {
            let got = cat.get(None, id).unwrap_or_else(|| panic!("{lang}: нет перевода «{id}»"));
            assert_ne!(got, id, "{lang}: не переведено «{id}»");
            assert_eq!(id.matches('%').count(), got.matches('%').count(), "{lang}: «{id}»");
        }
        if lang == "ru" {
            assert_eq!(cat.get(None, msgids[5]), Some(ru_not_covered));
        }
    }
}

/// Два блока в файле без `\n` в конце: снятие в порядке установки не
/// склеивает метку второго блока с последней строкой человека, и после
/// снятия обоих файл байт-в-байт прежний. В обратном порядке — тоже.
#[test]
fn two_blocks_removed_in_install_order_keep_bytes() {
    let write = |e: TextEdit| match e {
        TextEdit::Write(t) => t,
        other => panic!("ждали Write, пришло {other:?}"),
    };
    for syntax in [Syntax::Sway, Syntax::HyprConf, Syntax::NiriKdl, Syntax::Shell] {
        let body = |id: &str| match syntax {
            Syntax::Sway => vec![format!("exec {id}")],
            Syntax::HyprConf => vec![format!("exec-once = {id}")],
            Syntax::NiriKdl => vec![format!("spawn-at-startup \"{id}\"")],
            _ => vec![format!("{id} &")],
        };
        let before = "first line\nlast line without newline";
        let one = write(ca::upsert_text(Some(before), syntax, "tray", &body("a"), &[]).unwrap());
        let two = write(ca::upsert_text(Some(&one), syntax, "session-env", &body("b"), &[]).unwrap());

        // В порядке установки.
        let mid = write(ca::remove_text(&two, syntax, "tray"));
        assert!(mid.starts_with(&format!("{before}\n")), "{syntax:?}: метка прилипла к строке человека:\n{mid}");
        assert!(ca::has_block(&mid, syntax, "session-env"), "{syntax:?}:\n{mid}");
        assert_eq!(write(ca::remove_text(&mid, syntax, "session-env")), before, "{syntax:?}");

        // В обратном порядке.
        let mid = write(ca::remove_text(&two, syntax, "session-env"));
        assert_eq!(mid, one, "{syntax:?}");
        assert_eq!(write(ca::remove_text(&mid, syntax, "tray")), before, "{syntax:?}");
    }
}

/// Заведённый нами файл с двумя блоками: снятие первого передаёт второму и
/// «файл наш» — после снятия обоих файл удаляется, а не остаётся пустым.
#[test]
fn two_blocks_in_our_new_file_delete_it_in_the_end() {
    let syntax = Syntax::Shell;
    let one = match ca::upsert_text(None, syntax, "tray", &["a &".into()], &[]).unwrap() {
        TextEdit::Write(t) => t,
        e => panic!("{e:?}"),
    };
    let two = match ca::upsert_text(Some(&one), syntax, "session-env", &["b &".into()], &[]).unwrap() {
        TextEdit::Write(t) => t,
        e => panic!("{e:?}"),
    };
    let mid = match ca::remove_text(&two, syntax, "tray") {
        TextEdit::Write(t) => t,
        e => panic!("{e:?}"),
    };
    assert_eq!(ca::remove_text(&mid, syntax, "session-env"), TextEdit::Delete, "{mid}");
}

/// Человек дописал строки после нашего блока: перевод строки перед ними
/// остаётся, иначе его строка склеилась бы с последней строкой до блока.
#[test]
fn foreign_lines_after_block_keep_their_newline() {
    let syntax = Syntax::Sway;
    let one = match ca::upsert_text(Some("set $mod Mod4"), syntax, "tray", &["exec a".into()], &[]).unwrap() {
        TextEdit::Write(t) => t,
        e => panic!("{e:?}"),
    };
    let edited = format!("{one}bindsym $mod+Return exec foot\n");
    assert_eq!(
        ca::remove_text(&edited, syntax, "tray"),
        TextEdit::Write("set $mod Mod4\nbindsym $mod+Return exec foot\n".into())
    );
}

/// Удаление Rexon снимает и блок переменных сеанса — в том числе из
/// `~/.xprofile` и `labwc/environment`, которых нет среди конфигов
/// композиторов, — и файлы возвращаются к прежним байтам.
#[test]
fn remove_all_rexon_takes_session_env_too() {
    assert!(ca::REXON_IDS.contains(&ca::ID_SESSION_ENV));
    let env = sandbox("remove_all_session_env");
    let body = vec!["export TERMINAL='kitty'".to_string()];
    let files = [
        (format!("{}/.xprofile", env.home), "xrdb -merge ~/.Xresources"),
        (format!("{}/labwc/environment", env.config_home), "XKB_DEFAULT_LAYOUT=us\n"),
    ];
    for (path, before) in &files {
        let with = match ca::upsert_text(Some(before), Syntax::Shell, ca::ID_SESSION_ENV, &body, &[]).unwrap() {
            TextEdit::Write(t) => t,
            e => panic!("{e:?}"),
        };
        put(path, &with);
    }
    let done = ca::remove_all_rexon(&env);
    assert_eq!(done.len(), files.len(), "{done:?}");
    for (path, before) in &files {
        assert_eq!(read(path), *before, "{path}");
    }
}

fn written(e: TextEdit) -> String {
    match e {
        TextEdit::Write(t) => t,
        other => panic!("ждали Write, пришло {other:?}"),
    }
}

/// Файл sway завёл Rexon (блок с `+new` и `include` системного), за ним
/// встал второй наш блок. Снятие первого не уносит `include`: пока в файле
/// есть наши строки, без него sway остался бы без системного конфига и
/// клавиш. После снятия обоих файл удаляется.
#[test]
fn sway_include_stays_while_our_blocks_remain() {
    let syntax = Syntax::Sway;
    let include = vec!["include /etc/sway/config".to_string()];
    let one = written(ca::upsert_text(None, syntax, "tray", &["exec a".into()], &include).unwrap());
    let two = written(ca::upsert_text(Some(&one), syntax, "session-splash", &["exec b".into()], &include).unwrap());
    assert_eq!(two.matches("include /etc/sway/config").count(), 1, "{two}");

    let mid = written(ca::remove_text(&two, syntax, "tray"));
    assert!(mid.contains("include /etc/sway/config\n"), "include ушёл вместе с первым блоком:\n{mid}");
    assert!(mid.contains("exec b") && !mid.contains("exec a"), "{mid}");
    assert!(
        mid.find("include").unwrap() < mid.find("exec b").unwrap(),
        "include должен идти до строк запуска:\n{mid}"
    );
    // Повтор установки второго блока не дублирует include и ничего не меняет.
    assert_eq!(
        ca::upsert_text(Some(&mid), syntax, "session-splash", &["exec b".into()], &include).unwrap(),
        TextEdit::Unchanged
    );
    assert_eq!(ca::remove_text(&mid, syntax, "session-splash"), TextEdit::Delete, "{mid}");

    // Обратный порядок — как раньше: файл удаляется.
    let mid = written(ca::remove_text(&two, syntax, "session-splash"));
    assert_eq!(mid, one);
    assert_eq!(ca::remove_text(&mid, syntax, "tray"), TextEdit::Delete);

    // Человек дописал свои строки в наш файл: include остаётся перед ними.
    let edited = format!("{one}bindsym Mod4+Return exec foot\n");
    assert_eq!(
        written(ca::remove_text(&edited, syntax, "tray")),
        "include /etc/sway/config\nbindsym Mod4+Return exec foot\n"
    );
}

/// То же на диске через `sync_command`: заведённый `~/.config/sway/config`
/// с двумя нашими строками, снятие первой оставляет `include`.
#[test]
fn sway_created_config_keeps_include_on_disk() {
    let env = sandbox("sway_two_blocks");
    put(&format!("{}/sway/config", env.sysconf), "bindsym Mod4+Return exec foot\n");
    let path = format!("{}/sway/config", env.config_home);
    let mut e = env.clone();
    e.installed = vec![Compositor::Sway];
    ca::sync_command(&e, "tray", Some(&argv(TRAY)));
    ca::sync_command(&e, "session-splash", Some(&argv(&["/x/splash"])));
    ca::sync_command(&e, "tray", None);
    let t = read(&path);
    assert!(t.contains(&format!("include {}/sway/config\n", env.sysconf)), "{t}");
    assert!(t.contains("exec /x/splash"), "{t}");
    ca::sync_command(&e, "session-splash", None);
    assert!(!Path::new(&path).exists(), "заведённый файл не удалён");
}

/// labwc (свой `autostart` исполняет системный) и Wayfire (заголовок
/// `[autostart]` завёл первый блок): второй блок не прячется внутрь первого,
/// и преамбула переходит ко второму. Оба порядка снятия возвращают файл.
#[test]
fn preamble_moves_to_next_block_labwc_and_wayfire() {
    let cases: [(Syntax, Vec<String>, [&str; 2]); 2] = [
        (Syntax::Shell, vec![". /etc/xdg/labwc/autostart".into()], ["a &", "b &"]),
        (Syntax::WayfireIni, vec![], ["rexon_a = a", "rexon_b = b"]),
    ];
    for (syntax, pre, [a, b]) in cases {
        let one = written(ca::upsert_text(None, syntax, "tray", &[a.into()], &pre).unwrap());
        let two = written(ca::upsert_text(Some(&one), syntax, "session-splash", &[b.into()], &pre).unwrap());
        // Второй блок — целиком за первым, не внутри него.
        assert!(two.starts_with(&one), "{syntax:?}: второй блок лёг внутрь первого:\n{two}");
        let mid = written(ca::remove_text(&two, syntax, "tray"));
        let head = if syntax == Syntax::Shell { ". /etc/xdg/labwc/autostart" } else { "[autostart]" };
        assert!(mid.contains(head) && mid.find(head).unwrap() < mid.find(b).unwrap(), "{syntax:?}:\n{mid}");
        assert_eq!(ca::remove_text(&mid, syntax, "session-splash"), TextEdit::Delete, "{syntax:?}: {mid}");
    }
    // Wayfire, чужой файл без раздела: после снятия обоих — прежние байты.
    let before = "[core]\nplugins = autostart\n";
    let one = written(ca::upsert_text(Some(before), Syntax::WayfireIni, "tray", &["rexon_a = a".into()], &[]).unwrap());
    let two = written(ca::upsert_text(Some(&one), Syntax::WayfireIni, "session-splash", &["rexon_b = b".into()], &[]).unwrap());
    assert_eq!(two.matches("[autostart]").count(), 1, "{two}");
    let mid = written(ca::remove_text(&two, Syntax::WayfireIni, "tray"));
    assert!(mid.contains("[autostart]\nrexon_b = b"), "{mid}");
    assert_eq!(written(ca::remove_text(&mid, Syntax::WayfireIni, "session-splash")), before);
}
