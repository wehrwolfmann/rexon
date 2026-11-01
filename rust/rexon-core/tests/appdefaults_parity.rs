//! Golden-сверка порта `appdefaults.rs` с оригиналом `rexon/appdefaults.py`.
//!
//! Эталоны сняты с ЖИВОГО Python (генератор импортирует `rexon.appdefaults`,
//! подменяет `_desktop_exists`/`shutil.which`/`safe_run` на фикстуры и
//! ЗАПИСЫВАЕТ вход→выход: текст mimeapps.list, argv команд DE-настройки,
//! содержимое helpers.rc, тексты autostart/apply-скрипта, resolve-таблицы,
//! детект DE по окружению). Реальный HOME не тронут (генератор работает во
//! временном HOME, файлы читаются/удаляются в песочнице). Rust обязан выдать
//! те же результаты байт-в-байт.
//!
//! Множества «существующих .desktop» и «доступных бинарей» кодируются в
//! фикстуре режимами (all/none/only/all_except) и превращаются в замыкания
//! `exists`/`which` — те же, что видел Python.

mod i18n_pin;

use rexon_core::appdefaults as ad;
use serde_json::Value;
use std::collections::HashSet;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/appdefaults_cases.json")).unwrap()
}

fn sv(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

/// Замыкание exists/which из спецификации режима фикстуры.
fn spec_set(spec: &Value) -> (String, HashSet<String>) {
    let mode = spec["mode"].as_str().unwrap().to_string();
    let items: HashSet<String> = spec
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    (mode, items)
}

fn make_pred(spec: &Value) -> impl Fn(&str) -> bool {
    let (mode, items) = spec_set(spec);
    move |x: &str| match mode.as_str() {
        "all" => true,
        "none" => false,
        "only" => items.contains(x),
        "all_except" => !items.contains(x),
        _ => unreachable!(),
    }
}

fn which_pred(items: &[String]) -> impl Fn(&str) -> bool {
    let s: HashSet<String> = items.iter().cloned().collect();
    move |name: &str| s.contains(name)
}

#[test]
fn resolve_apps_parity() {
    let d = fx();
    for (de, expected) in d["resolve_apps"].as_object().unwrap() {
        let got = ad::resolve_de_apps(de);
        let got_json: Vec<Vec<String>> = got
            .iter()
            .map(|(k, v)| vec![k.to_string(), v.clone()])
            .collect();
        let exp: Vec<Vec<String>> = expected
            .as_array()
            .unwrap()
            .iter()
            .map(sv)
            .collect();
        assert_eq!(got_json, exp, "resolve_de_apps({de})");
    }
}

#[test]
fn mimeapps_filename_and_tiling_parity() {
    let d = fx();
    for (de, exp) in d["mimeapps_filename"].as_object().unwrap() {
        assert_eq!(ad::resolve_mimeapps_filename(de), exp.as_str().unwrap(),
                   "filename({de})");
    }
    for (de, exp) in d["is_tiling"].as_object().unwrap() {
        assert_eq!(ad::is_tiling_wm(de), exp.as_bool().unwrap(), "is_tiling({de})");
    }
}

#[test]
fn detect_current_de_parity() {
    let d = fx();
    for c in d["detect"].as_array().unwrap() {
        let got = ad::detect_current_de(
            c["xdg"].as_str().unwrap(),
            c["session"].as_str().unwrap(),
            c["wayland"].as_bool().unwrap(),
        );
        let exp = c["result"].as_str().map(|s| s.to_string());
        assert_eq!(got, exp, "detect({c})");
    }
}

#[test]
fn fm_short_name_parity() {
    let d = fx();
    for c in d["fm_short"].as_array().unwrap() {
        let got = ad::fm_short_name(c["fm_id"].as_str().unwrap(),
                                    c["desktop"].as_str().unwrap());
        assert_eq!(got, c["result"].as_str().unwrap(), "fm_short({c})");
    }
}

#[test]
fn default_file_manager_parity() {
    let d = fx();
    for c in d["default_fm"].as_array().unwrap() {
        let which_items = sv(&c["which"]);
        let which = which_pred(&which_items);
        let de = c["de"].as_str();
        let got = ad::default_file_manager_for_de(de, &which);
        assert_eq!(got, c["result"].as_str().unwrap(), "default_fm({c})");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn generate_profile_parity() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let d = fx();
    for c in d["generate"].as_array().unwrap() {
        let de = c["de_id"].as_str().unwrap();
        let exists = make_pred(&c["exists"]);
        let got = ad::generate_profile(de, &exists);
        let out = &c["outcome"];
        match out["kind"].as_str().unwrap() {
            "skip" => assert_eq!(
                got,
                ad::ProfileOutcome::Skip { message: out["message"].as_str().unwrap().into() },
                "generate skip {de}"
            ),
            "noapps" => assert_eq!(
                got,
                ad::ProfileOutcome::NoApps { message: out["message"].as_str().unwrap().into() },
                "generate noapps {de}"
            ),
            "write" => assert_eq!(
                got,
                ad::ProfileOutcome::Write {
                    filename: out["filename"].as_str().unwrap().into(),
                    content: out["content"].as_str().unwrap().into(),
                    count: out["count"].as_u64().unwrap() as usize,
                },
                "generate write {de} exists={}", c["exists"]
            ),
            k => panic!("неизвестный kind {k}"),
        }
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn apply_de_specific_parity() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let d = fx();
    for c in d["apply"].as_array().unwrap() {
        let de = c["de_id"].as_str().unwrap();
        let exists = make_pred(&c["exists"]);
        let which_items = sv(&c["which"]);
        let which = which_pred(&which_items);
        let xfce_existing = c["xfce_existing"].as_str();
        let got = ad::apply_de_specific(de, &exists, &which, xfce_existing);

        let res = &c["result"];
        let exp_cmds: Vec<Vec<String>> = res["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(sv)
            .collect();
        assert_eq!(got.commands, exp_cmds, "apply commands {de} ({c})");
        let exp_helpers = res["helpers_rc"].as_str().map(|s| s.to_string());
        assert_eq!(got.helpers_rc, exp_helpers, "apply helpers {de}");
        assert_eq!(got.logs, sv(&res["logs"]), "apply logs {de}");
    }
}

/// LXQt: терминал из общей таблицы — `lxqt-qtxdg.conf` (libqtxdg), путь
/// раскрывает исполнитель. Намеренные расхождения с Python в эталоне:
/// GNOME/Budgie получают и `exec-arg`, LXQt — запись и строку журнала.
#[test]
fn apply_de_specific_lxqt_and_shared_terminal_table() {
    let all = |_: &str| true;
    let got = ad::apply_de_specific("lxqt", &all, &all, None);
    assert!(got.commands.is_empty());
    assert_eq!(
        got.file_steps,
        vec![rexon_core::defaults::TermStep::SetIni {
            path: "~/.config/lxqt-qtxdg.conf".into(),
            section: "[General]".into(),
            key: "TerminalEmulator".into(),
            value: "qterminal.desktop".into(),
        }]
    );
    // Команды Cinnamon/MATE/KDE/GNOME — ровно Run-шаги общей таблицы.
    for (de, home) in [
        ("cinnamon-mint", rexon_core::defaults::TerminalHome::Cinnamon),
        ("mate", rexon_core::defaults::TerminalHome::Mate),
        ("kde", rexon_core::defaults::TerminalHome::Kde),
        ("gnome", rexon_core::defaults::TerminalHome::Gnome),
    ] {
        let got = ad::apply_de_specific(de, &all, &all, None);
        let term = ad::de_apps_info(de, &all)
            .into_iter()
            .find(|(c, _)| c == "terminal")
            .and_then(|(_, t)| t)
            .unwrap();
        let want: Vec<Vec<String>> = rexon_core::defaults::terminal_home_steps(home, &term, "~/.config")
            .into_iter()
            .filter_map(|s| match s {
                rexon_core::defaults::TermStep::Run { argv } => Some(argv),
                _ => None,
            })
            .collect();
        assert_eq!(got.commands, want, "{de}");
    }
}

#[test]
fn installed_des_parity() {
    let d = fx();
    for c in d["installed_des"].as_array().unwrap() {
        let exists = make_pred(&c["exists"]);
        let got = ad::installed_des(&exists);
        assert_eq!(got, sv(&c["result"]), "installed_des ({c})");
    }
}

#[test]
fn de_apps_info_parity() {
    let d = fx();
    for c in d["de_apps_info"].as_array().unwrap() {
        let de = c["de_id"].as_str().unwrap();
        let exists = make_pred(&c["exists"]);
        let got = ad::de_apps_info(de, &exists);
        let got_json: Vec<Value> = got
            .iter()
            .map(|(cat, resolved)| {
                serde_json::json!([cat, resolved])
            })
            .collect();
        assert_eq!(&Value::Array(got_json), &c["result"], "de_apps_info {de}");
    }
}

#[test]
fn autostart_parity() {
    let d = fx();
    // Переснято 01.10.2026: `Comment` — английский + `Comment[<код>]` из
    // каталога (прежде был только русский, без ключа языка).
    for c in d["autostart"].as_array().unwrap() {
        let got = i18n_pin::ru_only(&ad::autostart_desktop_content(c["exec_path"].as_str().unwrap()));
        assert_eq!(got, c["content"].as_str().unwrap(), "autostart");
    }
}

#[test]
fn fm_default_and_fm1_parity() {
    let d = fx();
    assert_eq!(ad::FM1_NAME, d["fm1_name"].as_str().unwrap());
    for c in d["fm_default"].as_array().unwrap() {
        let exists = make_pred(&c["exists"]);
        let got = ad::fm_default_target(c["fm_id"].as_str().unwrap(), &exists);
        let exp = c["target"].as_str().map(|s| s.to_string());
        assert_eq!(got, exp, "fm_default_target ({c})");
        if let Some(target) = &got {
            let exp_cmd = sv(&c["xdg_cmd"]);
            assert_eq!(ad::xdg_mime_default_cmd(target), exp_cmd, "xdg_cmd");
        }
    }
    assert_eq!(ad::xdg_mime_query_cmd(), sv(&d["xdg_query_cmd"]));

    for c in d["fm1"].as_array().unwrap() {
        let txt = c["txt"].as_str().unwrap();
        let binary = c["binary"].as_str().unwrap();
        assert_eq!(ad::fm1_matches(txt, binary), c["matches"].as_bool().unwrap(),
                   "fm1_matches ({c})");
        let exp_base = c["exec_basename"].as_str().map(|s| s.to_string());
        assert_eq!(ad::fm1_exec_basename(txt), exp_base, "fm1_exec_basename ({c})");
    }
    for c in d["busctl_pid"].as_array().unwrap() {
        let got = ad::busctl_pid(c["stdout"].as_str().unwrap());
        let exp = c["pid"].as_i64();
        assert_eq!(got, exp, "busctl_pid ({c})");
    }
}

// ============================================================================
// Слот `org.freedesktop.FileManager1`
// ============================================================================
//
// Эталона на Python тут нет и быть не может: в оригинале этого решения не
// существовало вовсе — он умел только «PID владельца + SIGTERM, если его
// `comm` нашёлся в списке», и на Thunar не срабатывал никогда (`comm` =
// `Thunar`, список — строчный). Поэтому сверки ниже — не golden, а разбор
// веток: владелец совпал с умолчанием / чужой демон / имени никто не держит /
// у чужого открыты окна.

/// Живой вывод `busctl --user --no-pager status org.freedesktop.FileManager1`
/// (сокращён до строк, которые читает разбор).
const BUSCTL_THUNAR: &str = "PID=2583\nPIDFD=yes\nPPID=1231\nUID=1000\n\
Comm=Thunar\nExe=/usr/bin/Thunar\n\
CommandLine=/usr/bin/Thunar --sm-client-id 10188108 --daemon\n\
UniqueName=:1.64\n";

fn owner(stdout: &str, has_windows: Option<bool>) -> ad::Fm1Owner {
    ad::Fm1Owner {
        facts: ad::busctl_owner(stdout).expect("владелец разобран"),
        has_windows,
    }
}

#[test]
fn busctl_owner_reads_pid_comm_exe_and_cmdline() {
    let facts = ad::busctl_owner(BUSCTL_THUNAR).expect("владелец");
    assert_eq!(facts.pid, 2583);
    assert_eq!(facts.comm, "Thunar");
    assert_eq!(facts.exe, "/usr/bin/Thunar");
    assert!(facts.cmdline.contains("--daemon"), "{}", facts.cmdline);
    assert_eq!(facts.display(), "Thunar");
    // Имя никто не держит — шина не печатает PID.
    assert_eq!(ad::busctl_owner("Failed to get bus name: no such name\n"), None);
}

/// ⚠️ Тот самый дефект: `/proc/<pid>/comm` у Thunar — с большой буквы.
#[test]
fn process_name_is_matched_regardless_of_case() {
    assert_eq!(ad::fm_short_from_process("Thunar", "/usr/bin/Thunar").as_deref(), Some("thunar"));
    assert_eq!(ad::fm_short_from_process("nautilus", "").as_deref(), Some("nautilus"));
    // Comm обрезан ядром до 15 символов — выручает Exe.
    assert_eq!(
        ad::fm_short_from_process("cosmic-files-a", "/usr/bin/cosmic-files").as_deref(),
        Some("cosmic-files")
    );
    assert_eq!(ad::fm_short_from_process("kwin_wayland", "/usr/bin/kwin_wayland"), None);
}

/// СТОРОЖ полноты (правило #12): у каждого известного продукту файлового
/// менеджера обязана быть запись «как гасится / чем выдаёт режим службы».
/// Иначе новый менеджер молча попадёт в ветку «не трогаем» навсегда.
#[test]
fn guard_every_known_file_manager_has_daemon_row() {
    for short in ad::FM_SHORT_NAMES {
        let info = ad::fm_daemon_info(short)
            .unwrap_or_else(|| panic!("нет записи FM_DAEMONS для {short}"));
        assert_eq!(info.short, *short);
        assert!(!info.daemon_flags.is_empty(), "{short}: нечем узнать режим службы");
    }
    assert_eq!(ad::FM_DAEMONS.len(), ad::FM_SHORT_NAMES.len(), "таблицы разошлись");
}

#[test]
fn verdict_free_when_nobody_holds_the_name() {
    assert_eq!(ad::fm1_verdict(None, "dolphin"), ad::Fm1Verdict::Free);
}

#[test]
fn verdict_owned_when_owner_is_the_default() {
    let o = owner("PID=7\nComm=dolphin\nExe=/usr/bin/dolphin\nCommandLine=/usr/bin/dolphin\n", None);
    assert_eq!(
        ad::fm1_verdict(Some(&o), "dolphin"),
        ad::Fm1Verdict::Owned { short: "dolphin".into(), pid: 7 }
    );
}

#[test]
fn verdict_unknown_when_owner_is_not_a_file_manager() {
    let o = owner("PID=9\nComm=gvfsd\nExe=/usr/lib/gvfsd\nCommandLine=/usr/lib/gvfsd\n", None);
    assert_eq!(
        ad::fm1_verdict(Some(&o), "dolphin"),
        ad::Fm1Verdict::Unknown { process: "gvfsd".into(), pid: 9 }
    );
}

#[test]
fn verdict_evicts_foreign_daemon() {
    let o = owner(BUSCTL_THUNAR, None);
    assert_eq!(
        ad::fm1_verdict(Some(&o), "dolphin"),
        ad::Fm1Verdict::Evict {
            short: "thunar".into(),
            pid: 2583,
            // Гасим ИМЕННО бинарь владельца, а не одноимённый из PATH.
            quit: vec!["/usr/bin/Thunar".into(), "--quit".into()],
        }
    );
}

/// ⛔ У чужого менеджера открыты окна — там работа человека, не гасим.
#[test]
fn verdict_keeps_foreign_owner_that_has_windows() {
    // Даже с флагом службы в командной строке: наблюдение сильнее.
    let o = owner(BUSCTL_THUNAR, Some(true));
    assert_eq!(
        ad::fm1_verdict(Some(&o), "dolphin"),
        ad::Fm1Verdict::Busy { short: "thunar".into(), pid: 2583 }
    );

    // Окон нет — гасим, даже если флага службы в строке запуска не видно.
    let windowless = owner(
        "PID=11\nComm=nemo\nExe=/usr/bin/nemo\nCommandLine=/usr/bin/nemo\n",
        Some(false),
    );
    assert!(matches!(
        ad::fm1_verdict(Some(&windowless), "dolphin"),
        ad::Fm1Verdict::Evict { .. }
    ));

    // Наблюдения нет (Wayland) и флага службы нет — считаем, что окно есть.
    let unknown_windows = owner(
        "PID=11\nComm=nemo\nExe=/usr/bin/nemo\nCommandLine=/usr/bin/nemo\n",
        None,
    );
    assert_eq!(
        ad::fm1_verdict(Some(&unknown_windows), "dolphin"),
        ad::Fm1Verdict::Busy { short: "nemo".into(), pid: 11 }
    );
}

/// У менеджера без своей команды выхода argv пустой — исполнитель гасит
/// процесс по PID.
#[test]
fn quit_argv_is_empty_when_manager_has_no_quit_option() {
    assert!(ad::fm_quit_argv("dolphin", "/usr/bin/dolphin").is_empty());
    assert_eq!(ad::fm_quit_argv("thunar", ""), vec!["thunar", "--quit"]);
}

// ── Сохранённая сессия KDE ───────────────────────────────────────────────

/// Живой `~/.config/ksmserverrc` с восстанавливаемым демоном Thunar.
const KSM: &str = "[General]\n\
excludeApps=\n\
\n\
[LegacySession: saved at previous logout]\n\
count=0\n\
\n\
[Session: saved at previous logout]\n\
clientId1=10188108\n\
count=2\n\
program1=firefox\n\
restartCommand1=/usr/lib/firefox/firefox,--sm-client-id,10188108\n\
userId1=user\n\
clientId2=10188109\n\
program2=Thunar\n\
restartCommand2=Thunar,--sm-client-id,10188109,--daemon\n\
restartStyleHint2=0\n\
userId2=user\n";

#[test]
fn ksmserver_forget_drops_entry_and_excludes_app() {
    let out = ad::ksmserver_forget(KSM, "thunar").expect("файл обязан измениться");
    assert!(out.contains("excludeApps=thunar"), "{out}");
    // Запись Thunar снята целиком — все ключи с её номером.
    assert!(!out.contains("Thunar"), "{out}");
    assert!(!out.contains("clientId2"), "{out}");
    assert!(!out.contains("restartStyleHint2"), "{out}");
    // Чужая запись не тронута, номера не сдвинуты.
    assert!(out.contains("program1=firefox"), "{out}");
    assert!(out.contains("restartCommand1=/usr/lib/firefox/firefox,--sm-client-id,10188108"), "{out}");
    // count оставлен как был — KDE пропускает запись с пустым program.
    assert!(out.contains("count=2"), "{out}");
    // Повторный прогон менять уже нечего.
    assert_eq!(ad::ksmserver_forget(&out, "thunar"), None);
}

#[test]
fn ksmserver_forget_keeps_other_excluded_apps() {
    let text = "[General]\nexcludeApps=nemo,caja\n\n[Session: saved at previous logout]\ncount=1\nprogram1=Thunar\n";
    let out = ad::ksmserver_forget(text, "thunar").expect("изменение");
    assert!(out.contains("excludeApps=nemo,caja,thunar"), "{out}");
}

#[test]
fn ksmserver_forget_adds_general_group_when_missing() {
    let text = "[Session: saved at previous logout]\ncount=1\nprogram1=nemo\n";
    let out = ad::ksmserver_forget(text, "nemo").expect("изменение");
    assert!(out.starts_with("[General]\nexcludeApps=nemo\n"), "{out}");
    assert!(!out.contains("program1"), "{out}");
}

#[test]
fn ksmserver_forget_leaves_unrelated_file_alone() {
    let text = "[General]\nexcludeApps=thunar\n\n[Session: saved at previous logout]\ncount=1\nprogram1=firefox\n";
    assert_eq!(ad::ksmserver_forget(text, "thunar"), None);
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Те же случаи по-английски: исходы те же, в сообщениях и журнале — ни
/// одной русской буквы.
#[test]
fn english_appdefaults_have_no_russian() {
    let _lang = i18n_pin::en();
    let d = fx();
    for c in d["generate"].as_array().unwrap() {
        let de = c["de_id"].as_str().unwrap();
        let exists = make_pred(&c["exists"]);
        match ad::generate_profile(de, &exists) {
            ad::ProfileOutcome::Skip { message } | ad::ProfileOutcome::NoApps { message } => {
                assert!(!has_cyrillic(&message), "{de}: {message}")
            }
            ad::ProfileOutcome::Write { content, .. } => {
                assert!(!has_cyrillic(&content), "{de}: {content}")
            }
        }
    }
    for c in d["apply"].as_array().unwrap() {
        let de = c["de_id"].as_str().unwrap();
        let exists = make_pred(&c["exists"]);
        let which_items = sv(&c["which"]);
        let which = which_pred(&which_items);
        let got = ad::apply_de_specific(de, &exists, &which, c["xfce_existing"].as_str());
        assert_eq!(got.logs.len(), sv(&c["result"]["logs"]).len(), "{de}");
        for line in &got.logs {
            assert!(!has_cyrillic(line), "{de}: {line}");
        }
    }
    assert_eq!(
        ad::generate_profile("kde", &|_: &str| false),
        ad::ProfileOutcome::Skip { message: "kde: not installed, skipping".into() }
    );
}
