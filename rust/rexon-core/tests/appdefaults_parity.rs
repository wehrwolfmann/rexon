//! Golden check of the `appdefaults.rs` port against the original `rexon/appdefaults.py`.
//!
//! The golden files were captured from LIVE Python (the generator imports `rexon.appdefaults`,
//! replaces `_desktop_exists`/`shutil.which`/`safe_run` with fixtures and
//! RECORDS input→output: the mimeapps.list text, argv of the DE setup commands,
//! the helpers.rc contents, the autostart/apply script texts, resolve tables,
//! DE detection from the environment). The real HOME was not touched (the generator works in a
//! temporary HOME, files are read/removed in a sandbox). Rust must produce
//! the same results byte for byte.
//!
//! The sets of "existing .desktop files" and "available binaries" are encoded in the
//! fixture as modes (all/none/only/all_except) and turned into the closures
//! `exists`/`which` — the same ones Python saw.

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

/// exists/which closure from the fixture's mode spec.
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
    // The golden values are the original's Russian texts: checked through po/ru.po.
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
    // The golden values are the original's Russian texts: checked through po/ru.po.
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

/// LXQt: the terminal from the common table — `lxqt-qtxdg.conf` (libqtxdg), the path
/// is expanded by the executor. Deliberate divergences from Python in the golden file:
/// GNOME/Budgie also get `exec-arg`, LXQt gets a write and a log line.
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
    // Cinnamon/MATE/KDE/GNOME commands — exactly the Run steps of the common table.
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
    // Recaptured 01.10.2026: `Comment` is English + `Comment[<code>]` from the
    // catalog (before, there was only Russian, without a language key).
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
// The `org.freedesktop.FileManager1` slot
// ============================================================================
//
// There is no Python golden file here and there cannot be: this solution did not
// exist in the original at all — it could only do "owner PID + SIGTERM if its
// `comm` is found in the list", and it never worked on Thunar (`comm` =
// `Thunar`, the list is lowercase). So the checks below are not golden but a walk of the
// branches: the owner matches the default / a foreign daemon / nobody holds the name /
// the foreign one has open windows.

/// Live output of `busctl --user --no-pager status org.freedesktop.FileManager1`
/// (cut down to the lines the parser reads).
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
    // Nobody holds the name — the bus prints no PID.
    assert_eq!(ad::busctl_owner("Failed to get bus name: no such name\n"), None);
}

/// ⚠️ That very defect: Thunar's `/proc/<pid>/comm` is capitalized.
#[test]
fn process_name_is_matched_regardless_of_case() {
    assert_eq!(ad::fm_short_from_process("Thunar", "/usr/bin/Thunar").as_deref(), Some("thunar"));
    assert_eq!(ad::fm_short_from_process("nautilus", "").as_deref(), Some("nautilus"));
    // Comm is cut by the kernel to 15 characters — Exe comes to the rescue.
    assert_eq!(
        ad::fm_short_from_process("cosmic-files-a", "/usr/bin/cosmic-files").as_deref(),
        Some("cosmic-files")
    );
    assert_eq!(ad::fm_short_from_process("kwin_wayland", "/usr/bin/kwin_wayland"), None);
}

/// Completeness GUARD (rule #12): every file manager known to the product
/// must have an entry "how it is stopped / how it shows service mode".
/// Otherwise a new manager silently lands in the "leave alone" branch forever.
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
            // We stop EXACTLY the owner's binary, not a same-named one from PATH.
            quit: vec!["/usr/bin/Thunar".into(), "--quit".into()],
        }
    );
}

/// ⛔ The foreign manager has open windows — that is the person's work, we do not stop it.
#[test]
fn verdict_keeps_foreign_owner_that_has_windows() {
    // Even with the service flag on the command line: observation wins.
    let o = owner(BUSCTL_THUNAR, Some(true));
    assert_eq!(
        ad::fm1_verdict(Some(&o), "dolphin"),
        ad::Fm1Verdict::Busy { short: "thunar".into(), pid: 2583 }
    );

    // No windows — we stop it, even if the service flag is not visible in the launch line.
    let windowless = owner(
        "PID=11\nComm=nemo\nExe=/usr/bin/nemo\nCommandLine=/usr/bin/nemo\n",
        Some(false),
    );
    assert!(matches!(
        ad::fm1_verdict(Some(&windowless), "dolphin"),
        ad::Fm1Verdict::Evict { .. }
    ));

    // No observation (Wayland) and no service flag — assume there is a window.
    let unknown_windows = owner(
        "PID=11\nComm=nemo\nExe=/usr/bin/nemo\nCommandLine=/usr/bin/nemo\n",
        None,
    );
    assert_eq!(
        ad::fm1_verdict(Some(&unknown_windows), "dolphin"),
        ad::Fm1Verdict::Busy { short: "nemo".into(), pid: 11 }
    );
}

/// For a manager without its own quit command argv is empty — the executor stops
/// the process by PID.
#[test]
fn quit_argv_is_empty_when_manager_has_no_quit_option() {
    assert!(ad::fm_quit_argv("dolphin", "/usr/bin/dolphin").is_empty());
    assert_eq!(ad::fm_quit_argv("thunar", ""), vec!["thunar", "--quit"]);
}

// ── Saved KDE session ────────────────────────────────────────────────────

/// A live `~/.config/ksmserverrc` with a restorable Thunar daemon.
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
    // The Thunar entry is removed entirely — all keys with its number.
    assert!(!out.contains("Thunar"), "{out}");
    assert!(!out.contains("clientId2"), "{out}");
    assert!(!out.contains("restartStyleHint2"), "{out}");
    // The foreign entry is untouched, numbers are not shifted.
    assert!(out.contains("program1=firefox"), "{out}");
    assert!(out.contains("restartCommand1=/usr/lib/firefox/firefox,--sm-client-id,10188108"), "{out}");
    // count is left as it was — KDE skips an entry with an empty program.
    assert!(out.contains("count=2"), "{out}");
    // A repeated run has nothing left to change.
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

/// The same cases in English: the outcomes are the same, and the messages and the log have
/// not a single Russian letter.
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
