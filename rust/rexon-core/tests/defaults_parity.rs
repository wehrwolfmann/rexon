//! Golden check of the `defaults.rs` port against the original `rexon/defaults.py`
//! (`SystemDefaultsProtector`).
//!
//! The golden files were captured from LIVE Python read-only (without privileged actions:
//! subprocess/safe_run mocked, HOME a temporary directory) — the generator
//! ran the protector's pure methods on realistic inputs and recorded
//! input→output into `golden/defaults_cases.json`. Covered: serialization of
//! snapshot/rules/pending, `_set_ini_value`, `_get_current_terminal`,
//! the `_set_terminal` plan, `_set_default`, `snapshot`, `restore_defaults`,
//! `get_status_summary`, `should_protect`, `resolve_choice`,
//! `_get_exec_from_desktop`, parsing/generating `*-mimeapps.list`,
//! `protect_de_mimeapps` + constant tables.

mod i18n_pin;

use std::collections::HashMap;

use rexon_core::defaults as D;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/defaults_cases.json")).unwrap()
}

// ── JSON → core type converters ──────────────────────────────────────────

/// Object {k: (str|null)} → ordered pairs (file order = Python order).
fn obj_pairs_opt(v: &Value) -> Vec<(String, Option<String>)> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, val)| (k.clone(), val.as_str().map(String::from)))
                .collect()
        })
        .unwrap_or_default()
}

/// Object {k: str} → ordered pairs.
fn obj_pairs(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, val)| (k.clone(), val.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Object {app_type: {"old":.., "new":..}} → pending pairs.
fn obj_pending(v: &Value) -> Vec<(String, (String, String))> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, c)| {
                    (
                        k.clone(),
                        (
                            c["old"].as_str().unwrap_or("").to_string(),
                            c["new"].as_str().unwrap_or("").to_string(),
                        ),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Rules from the case's overrides on top of the defaults (like make_prot in the generator).
fn rules_from(overrides: &Value) -> D::Rules {
    let map = overrides.as_object().cloned().unwrap_or_default();
    D::Rules::defaults().updated(&map)
}

// ── serialization of snapshot / rules / pending (byte for byte) ──────────

#[test]
fn serialize_snapshot_byte_identical() {
    let f = fx();
    let ts = "2026-07-21T03:10:00.123456";
    assert_eq!(D::serialize_snapshot(&[], ts), f["snapshot_empty"].as_str().unwrap());

    let full = vec![
        ("file_manager".to_string(), Some("org.kde.dolphin.desktop".to_string())),
        ("text_editor".to_string(), Some("org.kde.kate.desktop".to_string())),
        ("terminal".to_string(), Some("org.kde.konsole.desktop".to_string())),
    ];
    assert_eq!(D::serialize_snapshot(&full, ts), f["snapshot_full"].as_str().unwrap());
}

#[test]
fn serialize_rules_byte_identical() {
    let f = fx();
    assert_eq!(D::Rules::defaults().serialize(), f["rules_default"].as_str().unwrap());

    let mut over = serde_json::Map::new();
    over.insert("browser".into(), Value::Bool(true));
    over.insert("custom".into(), Value::Bool(false));
    let merged = D::Rules::defaults().updated(&over);
    assert_eq!(merged.serialize(), f["rules_merged"].as_str().unwrap());
}

#[test]
fn serialize_pending_byte_identical() {
    let f = fx();
    let pending = vec![
        ("browser".to_string(), ("firefox.desktop".to_string(), "chromium.desktop".to_string())),
        ("text_editor".to_string(), ("kate.desktop".to_string(), "gedit.desktop".to_string())),
    ];
    assert_eq!(D::serialize_pending(&pending), f["pending_full"].as_str().unwrap());
    assert_eq!(D::serialize_pending(&[]), f["pending_empty"].as_str().unwrap());
}

// ── _set_ini_value ───────────────────────────────────────────────────────

#[test]
fn set_ini_value_parity() {
    for c in fx()["ini"].as_array().unwrap() {
        let initial = c["initial"].as_str();
        let got = D::set_ini_value(
            initial,
            c["section"].as_str().unwrap(),
            c["key"].as_str().unwrap(),
            c["value"].as_str().unwrap(),
        );
        assert_eq!(got, c["result"].as_str().unwrap(), "ini case {c:?}");
    }
}

// ── _get_current_terminal ─────────────────────────────────────────────────

#[test]
fn get_current_terminal_parity() {
    for c in fx()["get_terminal"].as_array().unwrap() {
        let got = D::get_current_terminal(
            c["xdg_list"].as_str(),
            c["de"].as_str().unwrap(),
            c["kdeglobals"].as_str(),
            c["gsettings"].as_str(),
        );
        let want = c["result"].as_str().map(String::from);
        assert_eq!(got, want, "terminal case {c:?}");
    }
}

// ── our own terminal walk + the xdg-terminal-exec warning ─────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn pick_terminal_parity() {
    // The Python golden value is the Russian text of the warning.
    let _lang = i18n_pin::ru();
    let fx = fx();
    for (name, c) in fx["pick_terminal"].as_object().unwrap() {
        let available: Vec<String> = c["available"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let desktops: Vec<String> = c["desktops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            D::pick_terminal(&available, &desktops),
            c["picked"].as_str().map(String::from),
            "случай {name}"
        );
    }

    let note = &fx["terminal_note"];
    assert_eq!(D::terminal_status_note(true), note["present"].as_str().unwrap());
    assert_eq!(
        D::terminal_status_note(false),
        note["missing"].as_str().unwrap()
    );

    // Every candidate gets at least one .desktop name, otherwise `pick_terminal`
    // could not return even the canonical id.
    for (binary, ids) in D::TERMINAL_CANDIDATES {
        assert!(!ids.is_empty(), "у {binary} нет ни одного .desktop");
    }
}

// ── _set_terminal (plan → folded runs + files) ───────────────────────────

#[test]
fn set_terminal_plan_parity() {
    const HOME: &str = "/H";
    const CFG_PREFIX: &str = "/H/.config/";
    for c in fx()["set_terminal"].as_array().unwrap() {
        // The system has exo helpers (xfce4-terminal gets them from the
        // xfce4-settings package) — the check does not depend on the machine's contents.
        let plan = D::set_terminal_plan_with(
            c["terminal_desktop"].as_str().unwrap(),
            c["de"].as_str().unwrap(),
            HOME,
            &|_| true,
        );
        // We fold the plan into (runs, files) the way the executor would.
        let mut runs: Vec<Vec<String>> = Vec::new();
        let mut files: HashMap<String, String> = HashMap::new();
        for step in &plan {
            match step {
                D::TermStep::Run { argv } => runs.push(argv.clone()),
                D::TermStep::WriteFile { path, content } => {
                    files.insert(path.clone(), content.clone());
                }
                D::TermStep::MakeDirs { .. } => {}
                D::TermStep::SetIni { path, section, key, value } => {
                    let cur = files.get(path).map(String::as_str);
                    files.insert(path.clone(), D::set_ini_value(cur, section, key, value));
                }
            }
        }
        // Expected runs.
        let want_runs: Vec<Vec<String>> = c["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
            .collect();
        assert_eq!(runs, want_runs, "runs for {}", c["de"]);

        // Expected files (relative paths in the golden file).
        let want_files: HashMap<String, String> = c["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let got_files: HashMap<String, String> = files
            .into_iter()
            .map(|(k, v)| (k.strip_prefix(CFG_PREFIX).unwrap().to_string(), v))
            .collect();
        assert_eq!(got_files, want_files, "files for {}", c["de"]);
    }
}

// ── Terminal: one table "desktop → native place" ─────────────────────────
//
// Deliberate divergences from the Python golden file (the golden file was edited item by item):
//   * `exec-arg` of the GNOME schema — by terminal: `--` for gnome-terminal (its `-e`
//     is deprecated), `-x` for mate-terminal, otherwise `-e`;
//   * MATE: as a second step, `exec-arg` of its own schema (`-x` for mate-terminal);
//   * XFCE: `helpers.rc TerminalEmulator=` — was not written at all before.

/// Fold the plan into "desktop → what is written" (runs + files with keys).
fn term_table(desktop: &str, term: &str) -> Vec<String> {
    D::set_terminal_plan_with(term, desktop, "/H", &|_| true)
        .into_iter()
        .filter_map(|s| match s {
            D::TermStep::Run { argv } => Some(format!("run:{}", argv[2..].join(" "))),
            D::TermStep::SetIni { path, section, key, value } => {
                Some(format!("ini:{}|{section}|{key}={value}", path.trim_start_matches("/H/.config/")))
            }
            D::TermStep::WriteFile { path, content } => {
                Some(format!("file:{}|{}", path.trim_start_matches("/H/.config/"), content.trim_end()))
            }
            D::TermStep::MakeDirs { .. } => None,
        })
        .collect()
}

#[test]
fn terminal_written_to_native_place_of_every_desktop() {
    // The common layer: the common list, the desktop's own list (with the mark "chosen
    // by the person" — only a new choice overrides it), the GNOME schema, TERMINAL.
    let common = |de: &str, exec: &str, arg: &str, id: &str| {
        let mut v = vec![format!("file:xdg-terminals.list|{id}")];
        if let Some(name) = rexon_core::appdefaults::terminals_list_filename(de) {
            v.push(format!("file:{name}|{id}\n{}", D::TERMINALS_LIST_CHOSEN_MARK));
        }
        v.extend([
            format!("run:org.gnome.desktop.default-applications.terminal exec {exec}"),
            format!("run:org.gnome.desktop.default-applications.terminal exec-arg {arg}"),
            format!("file:environment.d/rexon-terminal.conf|TERMINAL={exec}"),
        ]);
        v
    };
    let with = |mut v: Vec<String>, extra: &[&str]| {
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    // Cinnamon — both by the raw XDG_CURRENT_DESKTOP and by the Rexon identifier.
    for de in ["X-Cinnamon", "cinnamon-mint", "cinnamon"] {
        assert_eq!(
            term_table(de, "org.gnome.Terminal.desktop"),
            with(
                common(de, "gnome-terminal", "--", "org.gnome.Terminal.desktop"),
                &[
                    "run:org.cinnamon.desktop.default-applications.terminal exec gnome-terminal",
                    "run:org.cinnamon.desktop.default-applications.terminal exec-arg --",
                ]
            ),
            "{de}"
        );
    }
    assert_eq!(
        term_table("XFCE", "Alacritty.desktop"),
        with(common("XFCE", "alacritty", "-e", "Alacritty.desktop"), &["ini:xfce4/helpers.rc||TerminalEmulator=alacritty"])
    );
    for de in ["LXQt", "LXQt:labwc:wlroots", "lxqt"] {
        assert_eq!(
            term_table(de, "qterminal.desktop"),
            with(
                common(de, "qterminal", "-e", "qterminal.desktop"),
                &["ini:lxqt-qtxdg.conf|[General]|TerminalEmulator=qterminal.desktop"]
            ),
            "{de}"
        );
    }
    assert_eq!(
        term_table("MATE", "mate-terminal.desktop"),
        with(
            common("MATE", "mate-terminal", "-x", "mate-terminal.desktop"),
            &[
                "run:org.mate.applications-terminal exec mate-terminal",
                "run:org.mate.applications-terminal exec-arg -x",
            ]
        )
    );
    // GNOME: the native place = the common layer, no repeats.
    for de in ["GNOME", "ubuntu:GNOME"] {
        assert_eq!(term_table(de, "org.gnome.Console.desktop"), common(de, "kgx", "-e", "org.gnome.Console.desktop"), "{de}");
    }
    // Budgie: the native place is the same as GNOME's, plus the Cinnamon schema — its
    // «Открыть в терминале» is read by Nemo, the file manager of the Budgie set
    // (`defaults::terminal_extra_homes`).
    for de in ["Budgie:GNOME", "budgie"] {
        assert_eq!(
            term_table(de, "xfce4-terminal.desktop"),
            with(
                common(de, "xfce4-terminal", "-e", "xfce4-terminal.desktop"),
                &[
                    "run:org.cinnamon.desktop.default-applications.terminal exec xfce4-terminal",
                    "run:org.cinnamon.desktop.default-applications.terminal exec-arg -e",
                ]
            ),
            "{de}"
        );
    }
    assert_eq!(
        term_table("KDE", "org.kde.konsole.desktop"),
        with(
            common("KDE", "konsole", "-e", "org.kde.konsole.desktop"),
            &[
                "ini:kdeglobals|[General]|TerminalApplication=konsole",
                "ini:kdeglobals|[General]|TerminalService=org.kde.konsole.desktop",
                "run:kdeglobals --group General --key TerminalApplication konsole",
                "run:kdeglobals --group General --key TerminalService org.kde.konsole.desktop",
            ]
        )
    );
    // No place of its own — only the common layer, honestly without a "native" entry.
    for de in ["COSMIC", "sway", "Hyprland", "niri", ""] {
        assert_eq!(term_table(de, "foot.desktop"), common(de, "foot", "-e", "foot.desktop"), "{de}");
    }
}

/// Program name: an explicit table where the binary does not match the id's tail.
#[test]
fn terminal_exec_name_table() {
    for (id, exec) in [
        ("org.gnome.Console.desktop", "kgx"),
        ("org.gnome.Terminal.desktop", "gnome-terminal"),
        ("com.system76.CosmicTerm.desktop", "cosmic-term"),
        ("org.kde.konsole.desktop", "konsole"),
        ("Alacritty.desktop", "alacritty"),
        ("com.mitchellh.ghostty.desktop", "ghostty"),
        ("xfce4-terminal.desktop", "xfce4-terminal"),
    ] {
        assert_eq!(D::terminal_exec_name(id), exec, "{id}");
    }
}

/// The XFCE helpers.rc is a file without sections: the key is edited in place, foreign lines
/// and comments stay.
#[test]
fn sectionless_ini_value() {
    assert_eq!(D::set_ini_value(None, "", "TerminalEmulator", "kitty"), "TerminalEmulator=kitty\n");
    assert_eq!(
        D::set_ini_value(Some("# c\nWebBrowser=firefox\nTerminalEmulator=old\n"), "", "TerminalEmulator", "kitty"),
        "# c\nWebBrowser=firefox\nTerminalEmulator=kitty\n"
    );
    assert_eq!(
        D::set_ini_value(Some("WebBrowser=firefox"), "", "TerminalEmulator", "kitty"),
        "WebBrowser=firefox\nTerminalEmulator=kitty\n"
    );
    // If there are sections after all, the key goes before the first header.
    assert_eq!(
        D::set_ini_value(Some("[X]\nTerminalEmulator=keep\n"), "", "TerminalEmulator", "kitty"),
        "TerminalEmulator=kitty\n[X]\nTerminalEmulator=keep\n"
    );
}

// ── _set_default ──────────────────────────────────────────────────────────

#[test]
fn set_default_parity() {
    for c in fx()["set_default"].as_array().unwrap() {
        let app_type = c["app_type"].as_str().unwrap();
        let got = D::set_default(app_type, c["desktop"].as_str().unwrap());
        let want_runs: Vec<Vec<String>> = c["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
            .collect();
        let want_terminal = c["terminal"].get("terminal").and_then(Value::as_str);
        match got {
            D::SetDefault::Terminal(desk) => {
                assert_eq!(Some(desk.as_str()), want_terminal, "terminal {app_type}");
                assert!(want_runs.is_empty(), "terminal case has no xdg runs");
            }
            D::SetDefault::Xdg(cmds) => {
                assert_eq!(cmds, want_runs, "xdg runs for {app_type}");
                assert!(want_terminal.is_none(), "non-terminal has no terminal call");
            }
        }
    }
}

// ── snapshot() ────────────────────────────────────────────────────────────

#[test]
fn compute_snapshot_parity() {
    for c in fx()["snapshot_fn"].as_array().unwrap() {
        let rules = rules_from(&c["rules"]);
        let existing = obj_pairs_opt(&c["protected"]);
        let current = obj_pairs_opt(&c["current"]);
        let got = D::compute_snapshot(&current, &rules, &existing);
        let want = obj_pairs_opt(&c["result"]);
        assert_eq!(got, want, "snapshot case {c:?}");
    }
}

// ── restore_defaults() ────────────────────────────────────────────────────

#[test]
fn restore_defaults_parity() {
    for c in fx()["restore"].as_array().unwrap() {
        let input = &c["input"];
        let rules = rules_from(&input["rules"]);
        let protected = obj_pairs_opt(&input["protected"]);
        let current = obj_pairs_opt(&input["current"]);
        let de_switched = input["de_switched"].as_bool().unwrap();
        let existing: Vec<String> = input["existing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        let exists = |d: &str| existing.iter().any(|e| e == d);

        let out = D::restore_defaults(&protected, &rules, &current, de_switched, &[], &exists);

        // results (ordered as in a Python dict).
        let want_results = obj_pairs(&c["results"]);
        assert_eq!(out.results, want_results, "restore results {c:?}");

        // set_calls.
        let want_calls: Vec<(String, String)> = c["set_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let p = a.as_array().unwrap();
                (p[0].as_str().unwrap().to_string(), p[1].as_str().unwrap().to_string())
            })
            .collect();
        assert_eq!(out.set_calls, want_calls, "restore set_calls {c:?}");

        // pending.
        assert_eq!(out.pending, obj_pending(&c["pending"]), "restore pending {c:?}");
    }
}

// ── get_status_summary ────────────────────────────────────────────────────

#[test]
fn status_summary_parity() {
    for c in fx()["status"].as_array().unwrap() {
        let rules = rules_from(&c["rules"]);
        let protected = obj_pairs_opt(&c["protected"]);
        let current = obj_pairs_opt(&c["current"]);
        let got = D::get_status_summary(&protected, &current, &rules);
        assert_eq!(got, c["result"], "status case {c:?}");
    }
}

// ── should_protect ────────────────────────────────────────────────────────

#[test]
fn should_protect_parity() {
    for c in fx()["should_protect"].as_array().unwrap() {
        let rules = rules_from(&c["rules"]);
        let protected = obj_pairs_opt(&c["protected"]);
        let existing: Vec<String> = c["existing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        let exists = |d: &str| existing.iter().any(|e| e == d);
        let got = D::should_protect(c["app_type"].as_str().unwrap(), &rules, &protected, &exists);
        assert_eq!(got, c["result"].as_bool().unwrap(), "should_protect {c:?}");
    }
}

// ── resolve_choice ────────────────────────────────────────────────────────

#[test]
fn resolve_choice_parity() {
    for c in fx()["resolve"].as_array().unwrap() {
        let input = &c["input"];
        let protected = obj_pairs_opt(&input["protected"]);
        let pending = obj_pending(&input["pending"]);
        let out = D::resolve_choice(
            input["app_type"].as_str().unwrap(),
            input["use_new"].as_bool().unwrap(),
            &protected,
            &pending,
        );
        // set_call → a list (like set_calls in the golden file).
        let want_calls: Vec<(String, String)> = c["set_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let p = a.as_array().unwrap();
                (p[0].as_str().unwrap().to_string(), p[1].as_str().unwrap().to_string())
            })
            .collect();
        let got_calls: Vec<(String, String)> = out.set_call.into_iter().collect();
        assert_eq!(got_calls, want_calls, "resolve set_call {c:?}");
        assert_eq!(out.protected_apps, obj_pairs_opt(&c["protected"]), "resolve protected {c:?}");
        assert_eq!(out.pending, obj_pending(&c["pending"]), "resolve pending {c:?}");
    }
}

// ── _get_exec_from_desktop ────────────────────────────────────────────────

#[test]
fn get_exec_from_desktop_parity() {
    for c in fx()["get_exec"].as_array().unwrap() {
        let got = D::get_exec_from_desktop(c["text"].as_str());
        let want = c["result"].as_str().map(String::from);
        assert_eq!(got, want, "get_exec {c:?}");
    }
}

// ── mimeapps parse / serialize ────────────────────────────────────────────

#[test]
fn mimeapps_parse_parity() {
    for c in fx()["mimeapps_parse"].as_array().unwrap() {
        let got = D::parse_mimeapps_list(c["text"].as_str().unwrap());
        assert_eq!(got, obj_pairs(&c["result"]), "mimeapps parse {c:?}");
    }
}

#[test]
fn mimeapps_serialize_parity() {
    for c in fx()["mimeapps_serialize"].as_array().unwrap() {
        let entries = obj_pairs(&c["entries"]);
        let got = D::serialize_mimeapps_list(&entries, c["ts"].as_str().unwrap());
        assert_eq!(got, c["result"].as_str().unwrap(), "mimeapps serialize {c:?}");
    }
}

/// Rexon recognises its own per-desktop list by the English marker and by the
/// Russian header of older versions; a file of the person is not "ours".
#[test]
fn rexon_mimeapps_recognised_by_marker_and_old_header() {
    let new = D::serialize_mimeapps_list(&[("a/b".into(), "x.desktop".into())], "2026-11-25T10:00:00");
    assert!(new.starts_with("# Rexon: per-desktop defaults\n# Updated: 2026-11-25T10:00:00\n"), "{new}");
    assert!(!new.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)), "{new}");
    assert!(D::is_rexon_mimeapps(&new));
    let old = "# Автоматически создан Rexon для изоляции DE\n# Обновлён: 2026-11-25T00:09:01\n\n\
               [Default Applications]\nimage/jpeg=xviewer.desktop\n";
    assert!(D::is_rexon_mimeapps(old));
    assert!(!D::is_rexon_mimeapps("[Default Applications]\nimage/jpeg=gimp.desktop\n"));
    assert!(!D::is_rexon_mimeapps("# my own notes about Rexon DE\n[Default Applications]\n"));
}

/// Neither generator of per-desktop lists ever writes a browser type
/// (`text/html`, `x-scheme-handler/http(s)` …), for any desktop, even when
/// every program is installed; a browser line already in the file stays as is.
#[test]
fn per_desktop_lists_never_write_browser_types() {
    let all: Vec<String> = D::DE_NATIVE_APPS.iter().map(|(k, _)| k.to_string()).collect();
    let out = D::protect_de_mimeapps("kde", &all, "/H/.config", "ts", &|_| true, &|_| None);
    assert!(!out.writes.is_empty());
    for (path, text) in &out.writes {
        for (mime, _) in D::parse_mimeapps_list(text) {
            assert!(!D::is_browser_mime(&mime), "{path}: {mime}");
        }
    }
    let ids = [
        "kde", "gnome", "mate", "cosmic", "cinnamon-mint", "xfce", "budgie", "pantheon",
        "deepin", "lxqt", "hyprland", "niri", "sway", "river", "wayfire", "unknown-wm",
    ];
    for de in ids {
        if let rexon_core::appdefaults::ProfileOutcome::Write { content, .. } =
            rexon_core::appdefaults::generate_profile(de, &|_| true)
        {
            for (mime, _) in D::parse_mimeapps_list(&content) {
                assert!(!D::is_browser_mime(&mime), "{de}: {mime}");
            }
        }
    }
    // The person's (hand-fixed) browser line survives a rewrite untouched.
    let kept = D::protect_de_mimeapps(
        "kde",
        &["kde".to_string()],
        "/H/.config",
        "ts",
        &|_| true,
        &|_| Some("[Default Applications]\ntext/html=firefox.desktop\n".to_string()),
    );
    assert!(kept.writes[0].1.contains("\ntext/html=firefox.desktop\n"), "{}", kept.writes[0].1);
}

/// The person's choice stays first in their desktop.
#[test]
fn person_choice_is_not_hidden_by_desktop_list() {
    let ours = D::serialize_mimeapps_list(
        &[("image/jpeg".into(), "org.kde.gwenview.desktop".into()), ("text/html".into(), "org.kde.kate.desktop".into())],
        "ts",
    );
    // (a) «Restore as it was» / a choice in Settings follows into our list…
    let next = D::follow_choice_in_desktop_list(&ours, "image/jpeg", "gimp.desktop").unwrap();
    assert!(next.contains("\nimage/jpeg=gimp.desktop\n") && next.starts_with("# Rexon: per-desktop defaults\n"), "{next}");
    // …but not a browser type, not a type that is not there, not the person's own file.
    assert_eq!(D::follow_choice_in_desktop_list(&ours, "text/html", "firefox.desktop"), None);
    assert_eq!(D::follow_choice_in_desktop_list(&ours, "video/mp4", "vlc.desktop"), None);
    assert_eq!(D::follow_choice_in_desktop_list(&next, "image/jpeg", "gimp.desktop"), None);
    assert_eq!(
        D::follow_choice_in_desktop_list("[Default Applications]\nimage/jpeg=a.desktop\n", "image/jpeg", "b.desktop"),
        None
    );
    assert_eq!(D::session_desktop_lists("Budgie:GNOME"), vec!["budgie-mimeapps.list", "gnome-mimeapps.list"]);
    assert_eq!(D::session_desktop_lists("X-Cinnamon"), vec!["x-cinnamon-mimeapps.list"]);
    assert!(D::session_desktop_lists("").is_empty());

    // (b) a change of the shared list during the session is adopted.
    let before = vec![("image/jpeg".to_string(), "org.kde.gwenview.desktop".to_string())];
    let after = vec![
        ("image/jpeg".to_string(), "gimp.desktop".to_string()),
        ("text/html".to_string(), "firefox.desktop".to_string()),
    ];
    let adopted = D::adopt_session_choices(&ours, &before, &after).unwrap();
    assert!(adopted.contains("\nimage/jpeg=gimp.desktop\n") && adopted.contains("\ntext/html=org.kde.kate.desktop\n"), "{adopted}");
    assert_eq!(D::adopt_session_choices(&ours, &before, &before), None);
    let seen = D::ChoicesSeen { list: "kde-mimeapps.list".into(), general: after.clone() };
    assert_eq!(D::parse_choices_seen(&D::serialize_choices_seen(&seen)), Some(seen));
    assert_eq!(D::parse_choices_seen("{\"list\":\"../x\",\"general\":{}}"), None);

    // (c) the install step keeps an installed program already in the list.
    let existing = vec![("image/jpeg".to_string(), "gimp.desktop".to_string()), ("image/png".to_string(), "gone.desktop".to_string())];
    let native = vec![
        ("image/jpeg".to_string(), "org.kde.gwenview.desktop".to_string()),
        ("image/png".to_string(), "org.kde.gwenview.desktop".to_string()),
        ("image/gif".to_string(), "org.kde.gwenview.desktop".to_string()),
        ("text/html".to_string(), "org.kde.kate.desktop".to_string()),
    ];
    let merged = D::merge_native_entries(&existing, &native, &|d| d != "gone.desktop");
    assert_eq!(
        merged,
        vec![
            ("image/jpeg".to_string(), "gimp.desktop".to_string()),
            ("image/png".to_string(), "org.kde.gwenview.desktop".to_string()),
            ("image/gif".to_string(), "org.kde.gwenview.desktop".to_string()),
        ]
    );
}

// ── protect_de_mimeapps ───────────────────────────────────────────────────

/// `XDG_CURRENT_DESKTOP` each desktop sets (its session file `DesktopNames=` or
/// the compositor itself): GLib and `xdg-mime` read `<first, lowercase>-mimeapps.list`.
const XDG_NAMES: &[(&str, &str)] = &[
    ("cinnamon-mint", "X-Cinnamon"),
    ("xfce", "XFCE"),
    ("mate", "MATE"),
    ("gnome", "GNOME"),
    ("kde", "KDE"),
    ("budgie", "Budgie:GNOME"),
    ("pantheon", "Pantheon"),
    ("deepin", "DDE"),
    ("cosmic", "COSMIC"),
    ("lxqt", "LXQt"),
    ("wayfire", "wayfire"),
    ("hyprland", "Hyprland"),
    ("sway", "sway"),
    ("niri", "niri"),
    ("river", "river"),
];

/// One naming rule for both generators, for every desktop Rexon installs.
/// Before 25.11.2026 the install step wrote `cinnamon-mimeapps.list`, which no
/// program reads under `X-Cinnamon`.
#[test]
fn mimeapps_name_follows_xdg_current_desktop_for_every_desktop() {
    for meta in rexon_core::system::DE_META {
        let xdg = XDG_NAMES
            .iter()
            .find(|(id, _)| *id == meta.id)
            .unwrap_or_else(|| panic!("{}: no XDG_CURRENT_DESKTOP in the test table", meta.id))
            .1;
        let want = format!("{}-mimeapps.list", xdg.split(':').next().unwrap().to_lowercase());
        assert_eq!(rexon_core::appdefaults::resolve_mimeapps_filename(meta.id), want, "{}", meta.id);
        // The install step (protect_de_mimeapps) writes the same name.
        let out = D::protect_de_mimeapps("kde", &[meta.id.to_string()], "/H", "ts", &|_| true, &|_| None);
        for (path, _) in &out.writes {
            assert_eq!(path, &format!("/H/{want}"), "{}", meta.id);
        }
    }
}

/// A Rexon list under the old Cinnamon name moves to the right name; the
/// person's own file under the old name is never touched.
#[test]
fn misnamed_cinnamon_list_is_moved() {
    assert_eq!(D::MISNAMED_MIMEAPPS, &[("cinnamon-mimeapps.list", "x-cinnamon-mimeapps.list")]);
    let ours = D::serialize_mimeapps_list(&[("image/jpeg".into(), "xviewer.desktop".into())], "ts");
    assert_eq!(D::misnamed_mimeapps_fix(Some(&ours), None), D::MisnamedFix::Move { content: ours.clone() });
    assert_eq!(D::misnamed_mimeapps_fix(Some(&ours), Some("[Default Applications]\n")), D::MisnamedFix::DropOld);
    assert_eq!(D::misnamed_mimeapps_fix(Some("[Default Applications]\nimage/jpeg=gimp.desktop\n"), None), D::MisnamedFix::Nothing);
    assert_eq!(D::misnamed_mimeapps_fix(None, None), D::MisnamedFix::Nothing);
}

fn desktop_name_for_test(de_id: &str) -> String {
    rexon_core::appdefaults::resolve_mimeapps_filename(de_id)
        .trim_end_matches("-mimeapps.list")
        .to_string()
}

#[test]
fn protect_de_mimeapps_parity() {
    // The same list of "installed" applications as in the generator.
    let valid = [
        "org.kde.dolphin.desktop",
        "org.kde.kate.desktop",
        "org.gnome.Nautilus.desktop",
        "org.gnome.TextEditor.desktop",
        "thunar.desktop",
    ];
    let cfg = "/H/.config";
    let ts = "2026-07-21T03:10:00";

    for c in fx()["protect_de_mimeapps"].as_array().unwrap() {
        let installed: Vec<String> = c["installed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();

        // existing: de_id → content, converted to path → content.
        let mut existing_by_path: HashMap<String, String> = HashMap::new();
        for (de_id, content) in c["existing"].as_object().unwrap() {
            let name = desktop_name_for_test(de_id);
            existing_by_path.insert(format!("{cfg}/{name}-mimeapps.list"), content.as_str().unwrap().to_string());
        }

        let exists = |d: &str| valid.contains(&d);
        let read_existing = |p: &str| existing_by_path.get(p).cloned();

        let out = D::protect_de_mimeapps(
            "kde", &installed, cfg, ts, &exists, &read_existing,
        );

        // results.
        assert_eq!(out.results, obj_pairs(&c["results"]), "pdm results {c:?}");

        // writes → compared by basename with the golden `written`.
        let want_written: HashMap<String, String> = c["written"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let got_written: HashMap<String, String> = out
            .writes
            .into_iter()
            .map(|(path, content)| (path.rsplit('/').next().unwrap().to_string(), content))
            .collect();
        assert_eq!(got_written, want_written, "pdm written {c:?}");
    }
}

// ── constant tables ───────────────────────────────────────────────────────

#[test]
fn tables_match_python() {
    let f = fx();
    let t = &f["tables"];

    let mime: Vec<(String, String)> = t["MIME_TYPES"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (a[0].as_str().unwrap().to_string(), a[1].as_str().unwrap().to_string())
        })
        .collect();
    let got_mime: Vec<(String, String)> =
        D::MIME_TYPES.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(got_mime, mime, "MIME_TYPES");

    // The former DE_DESKTOP_NAMES table is gone: one naming rule
    // (`mimeapps_name_follows_xdg_current_desktop_for_every_desktop`).
    assert!(t.get("DE_DESKTOP_NAMES").is_none());

    // DE_NATIVE_APPS: the keys and contents of each DE.
    let want_native = t["DE_NATIVE_APPS"].as_object().unwrap();
    for (de_id, apps) in D::DE_NATIVE_APPS {
        let want: Vec<(String, String)> = want_native[*de_id]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                (a[0].as_str().unwrap().to_string(), a[1].as_str().unwrap().to_string())
            })
            .collect();
        let got: Vec<(String, String)> =
            apps.iter().map(|(m, d)| (m.to_string(), d.to_string())).collect();
        assert_eq!(got, want, "DE_NATIVE_APPS[{de_id}]");
    }
    // and the number of DEs matches.
    assert_eq!(D::DE_NATIVE_APPS.len(), want_native.len(), "DE_NATIVE_APPS count");
}

// ── interface language ──────────────────────────────────────────────────

/// Program type captions and the xdg-terminal-exec warning — before the
/// switch to the catalog (snapshot `ca42e74^`): in Russian they must stay
/// word for word the same.
const RU_BEFORE: &[(&str, &str)] = &[
    ("terminal", "🖥️ Терминал"),
    ("file_manager", "📁 Файловый менеджер"),
    ("text_editor", "📝 Текстовый редактор"),
    ("browser", "🌐 Браузер"),
    ("email", "📧 Почтовый клиент"),
    ("image_viewer", "🖼️ Просмотр изображений"),
    ("video_player", "🎬 Видеоплеер"),
    ("pdf_viewer", "📄 Просмотр PDF"),
    ("archive_manager", "📦 Архиватор"),
    ("music_player", "🎵 Музыкальный плеер"),
];

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn russian_texts_as_before() {
    let _lang = i18n_pin::ru();
    for (key, want) in RU_BEFORE {
        assert_eq!(D::app_type_label(key), *want, "{key}");
    }
    assert_eq!(D::APP_TYPE_LABELS.len(), RU_BEFORE.len());
    assert_eq!(D::app_type_label("нет_такого"), "нет_такого");
    assert_eq!(
        D::terminal_status_note(false),
        "Часть программ откроет свой терминал, а не выбранный: для общесистемного \
         выбора нужна утилита xdg-terminal-exec, а её в системе нет. Выбор Rexon \
         уже записан — установите пакет «xdg-terminal-exec», и он заработает везде."
    );
}

#[test]
fn english_defaults_have_no_russian() {
    let _lang = i18n_pin::en();
    let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
    for (key, _) in D::APP_TYPE_LABELS {
        let shown = D::app_type_label(key);
        assert!(!cyrillic(&shown), "{key}: {shown}");
    }
    assert_eq!(D::app_type_label("browser"), "🌐 Web Browser");
    assert_eq!(D::terminal_status_note(true), "");
    let note = D::terminal_status_note(false);
    assert!(!cyrillic(&note), "{note}");
    assert!(note.starts_with("Some apps will open their own terminal"), "{note}");
}

/// D7 (desktop check of 02.10.2026): in XFCE a terminal without an exo helper
/// (konsole, foot, kgx…) got a bare name in `helpers.rc`, and
/// `exo-open --launch TerminalEmulator` opened a chooser window. Now an own
/// helper `custom-TerminalEmulator` is created for it — like xfce4-settings does.
#[test]
fn xfce_terminal_without_system_helper_gets_custom_helper() {
    let only_alacritty = |n: &str| n == "alacritty";
    let plan = D::set_terminal_plan_with("org.kde.konsole.desktop", "XFCE", "/H", &only_alacritty);
    let helper = "/H/.local/share/xfce4/helpers/custom-TerminalEmulator.desktop";
    let text = plan
        .iter()
        .find_map(|s| match s {
            D::TermStep::WriteFile { path, content } if path == helper => Some(content.clone()),
            _ => None,
        })
        .expect("свой помощник");
    for line in [
        "Type=X-XFCE-Helper",
        "X-XFCE-Category=TerminalEmulator",
        "X-XFCE-Binaries=konsole;",
        "X-XFCE-Commands=konsole;",
        "X-XFCE-CommandsWithParameter=konsole -e %s;",
    ] {
        assert!(text.lines().any(|l| l == line), "нет строки {line}:\n{text}");
    }
    assert!(plan.contains(&D::TermStep::SetIni {
        path: "/H/.config/xfce4/helpers.rc".into(),
        section: String::new(),
        key: "TerminalEmulator".into(),
        value: "custom-TerminalEmulator".into(),
    }));
    // The helper directory is created BEFORE the file is written.
    let mk = plan.iter().position(|s| matches!(s, D::TermStep::MakeDirs { path } if path == "/H/.local/share/xfce4/helpers"));
    let wr = plan.iter().position(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path == helper));
    assert!(mk.unwrap() < wr.unwrap());

    // The system has a helper — its name is written, no own file.
    let plan = D::set_terminal_plan_with("Alacritty.desktop", "XFCE", "/H", &only_alacritty);
    assert!(!plan.iter().any(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path.contains("xfce4/helpers/"))));
    assert!(plan.contains(&D::TermStep::SetIni {
        path: "/H/.config/xfce4/helpers.rc".into(),
        section: String::new(),
        key: "TerminalEmulator".into(),
        value: "alacritty".into(),
    }));
    // Outside XFCE we create no helpers of our own.
    let plan = D::set_terminal_plan_with("org.kde.konsole.desktop", "KDE", "/H", &|_| false);
    assert!(!plan.iter().any(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path.contains("xfce4"))));
}

/// D7: the file manager from «Настройки» reaches the XFCE `helpers.rc` —
/// Thunar with its own helper (lowercase, like the `thunar.desktop` file), Dolphin and
/// Nemo with our `custom-FileManager`; outside XFCE there are no steps.
#[test]
fn xfce_file_manager_goes_to_helpers_rc() {
    let ad = |fm: &str, de: &str| {
        rexon_core::appdefaults::file_manager_home_steps(fm, de, "/H/.config", &|n: &str| n == "thunar")
    };
    let set = |v: &str| D::TermStep::SetIni {
        path: "/H/.config/xfce4/helpers.rc".into(),
        section: String::new(),
        key: "FileManager".into(),
        value: v.into(),
    };
    assert_eq!(ad("thunar.desktop", "XFCE"), vec![set("thunar")]);
    let dolphin = ad("org.kde.dolphin.desktop", "XFCE");
    assert_eq!(dolphin.last(), Some(&set("custom-FileManager")));
    let text = dolphin
        .iter()
        .find_map(|s| match s {
            D::TermStep::WriteFile { path, content }
                if path == "/H/.local/share/xfce4/helpers/custom-FileManager.desktop" =>
            {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("свой помощник");
    assert!(text.contains("X-XFCE-Category=FileManager\n"), "{text}");
    assert!(text.contains("X-XFCE-CommandsWithParameter=dolphin %s;\n"), "{text}");
    assert_eq!(ad("nemo.desktop", "XFCE").last(), Some(&set("custom-FileManager")));
    assert!(ad("org.kde.dolphin.desktop", "KDE").is_empty());
    assert!(ad("thunar.desktop", "GNOME").is_empty());
}

/// Pantheon: the elementary terminal is written into the GNOME schema with the binary name
/// `io.elementary.terminal` (the tail after the dot would give a non-existent
/// "terminal") and with `-x` — "the rest of the line as a command" (its `-e` takes
/// one string per flag). Reading — through the same GNOME schema.
#[test]
fn pantheon_terminal_goes_to_gnome_schema_with_real_binary() {
    use rexon_core::defaults as d;
    assert_eq!(d::terminal_home("Pantheon"), d::TerminalHome::Gnome);
    assert_eq!(d::terminal_exec_name("io.elementary.terminal.desktop"), "io.elementary.terminal");
    assert_eq!(d::terminal_exec_arg("io.elementary.terminal"), "-x");
    assert_eq!(
        d::terminal_exec_schema("Pantheon"),
        Some("org.gnome.desktop.default-applications.terminal")
    );
    assert_eq!(d::terminal_exec_schema("KDE"), None);
    assert_eq!(d::terminal_exec_schema("MATE"), Some("org.mate.applications-terminal"));
    assert_eq!(
        d::get_current_terminal(None, "Pantheon", None, Some("'io.elementary.terminal'\n")).as_deref(),
        Some("io.elementary.terminal.desktop")
    );
}

// ── The desktop's own terminal list (`<desktop>-xdg-terminals.list`) ─────
//
// `xdg-terminal-exec` reads `~/.config/<desktop>-xdg-terminals.list` BEFORE
// the common `xdg-terminals.list`. The 08.11.2026 case: in MATE a foreign
// `mate-xdg-terminals.list` with GNOME Terminal overrode everything Rexon wrote.

/// The list name: the first element of `XDG_CURRENT_DESKTOP` in lowercase; for
/// Rexon identifiers — the same desktop name as for `<desktop>-mimeapps.list`.
#[test]
fn desktop_terminals_list_names() {
    use rexon_core::appdefaults::terminals_list_filename as name;
    for (desktop, want) in [
        ("MATE", "mate-xdg-terminals.list"),
        ("mate", "mate-xdg-terminals.list"),
        ("KDE", "kde-xdg-terminals.list"),
        ("kde", "kde-xdg-terminals.list"),
        ("GNOME", "gnome-xdg-terminals.list"),
        ("ubuntu:GNOME", "ubuntu-xdg-terminals.list"),
        ("Budgie:GNOME", "budgie-xdg-terminals.list"),
        ("X-Cinnamon", "x-cinnamon-xdg-terminals.list"),
        ("cinnamon-mint", "x-cinnamon-xdg-terminals.list"),
        ("XFCE", "xfce-xdg-terminals.list"),
        ("LXQt:labwc:wlroots", "lxqt-xdg-terminals.list"),
        ("DDE", "dde-xdg-terminals.list"),
        ("deepin", "dde-xdg-terminals.list"),
        ("Pantheon", "pantheon-xdg-terminals.list"),
        ("COSMIC", "cosmic-xdg-terminals.list"),
        ("sway", "sway-xdg-terminals.list"),
    ] {
        assert_eq!(name(desktop).as_deref(), Some(want), "{desktop}");
    }
    assert_eq!(name(""), None);
}

/// Marks and reading: the person's choice is recognized, a foreign list and the "native" one are not;
/// the terminal is always the first line (it is read by `get_current_terminal`).
#[test]
fn desktop_terminals_list_marks() {
    let chosen = D::desktop_terminals_list_text("org.kde.konsole.desktop", true);
    let native = D::desktop_terminals_list_text("mate-terminal.desktop", false);
    assert_eq!(chosen, format!("org.kde.konsole.desktop\n{}\n", D::TERMINALS_LIST_CHOSEN_MARK));
    assert_eq!(D::chosen_terminal_in_list(&chosen).as_deref(), Some("org.kde.konsole.desktop"));
    assert_eq!(D::chosen_terminal_in_list(&native), None);
    assert_eq!(D::chosen_terminal_in_list("org.gnome.Terminal.desktop\n"), None);
    assert_eq!(
        D::get_current_terminal(Some(&native), "MATE", None, None).as_deref(),
        Some("mate-terminal.desktop")
    );
    // The desktop's own list beats the common one; an empty one does not count.
    let s = |v: &str| Some(v.to_string());
    assert_eq!(D::effective_terminals_list(s("a.desktop\n"), s("b.desktop\n")), s("a.desktop\n"));
    assert_eq!(D::effective_terminals_list(s("\n"), s("b.desktop\n")), s("b.desktop\n"));
    assert_eq!(D::effective_terminals_list(None, s("b.desktop\n")), s("b.desktop\n"));
}

/// Session login: EVERY desktop with a native terminal gets it into its own list
/// (the same terminal that is written into the desktop's native place), a foreign list
/// is rewritten, the person's choice stays, tiling ones have no list.
#[test]
fn login_puts_native_terminal_into_desktop_list_of_every_desktop() {
    use rexon_core::appdefaults::{native_terminals_list_plan as plan, TerminalsListPlan as P};
    let all = |_: &str| true;
    let foreign = Some("org.gnome.Terminal.desktop\n");
    for (de_id, desktop, terminal, file) in [
        ("kde", "KDE", "org.kde.konsole.desktop", "kde-xdg-terminals.list"),
        ("gnome", "GNOME", "org.gnome.Console.desktop", "gnome-xdg-terminals.list"),
        ("mate", "MATE", "mate-terminal.desktop", "mate-xdg-terminals.list"),
        ("cinnamon-mint", "X-Cinnamon", "org.gnome.Terminal.desktop", "x-cinnamon-xdg-terminals.list"),
        ("xfce", "XFCE", "xfce4-terminal.desktop", "xfce-xdg-terminals.list"),
        ("budgie", "Budgie:GNOME", "xfce4-terminal.desktop", "budgie-xdg-terminals.list"),
        ("pantheon", "Pantheon", "io.elementary.terminal.desktop", "pantheon-xdg-terminals.list"),
        ("deepin", "DDE", "deepin-terminal.desktop", "dde-xdg-terminals.list"),
        ("lxqt", "LXQt", "qterminal.desktop", "lxqt-xdg-terminals.list"),
        ("cosmic", "COSMIC", "com.system76.CosmicTerm.desktop", "cosmic-xdg-terminals.list"),
    ] {
        let want = P::Write {
            filename: file.to_string(),
            terminal: terminal.to_string(),
            content: D::desktop_terminals_list_text(terminal, false),
        };
        // The same native terminal as in the desktop's set.
        let apps = rexon_core::appdefaults::resolve_de_apps(de_id);
        assert!(apps.iter().any(|(k, v)| *k == "terminal" && v == terminal), "{de_id}: {apps:?}");
        assert_eq!(plan(de_id, desktop, None, &all), want, "{de_id}: файла нет");
        if terminal != "org.gnome.Terminal.desktop" {
            assert_eq!(plan(de_id, desktop, foreign, &all), want, "{de_id}: чужой список");
        }
        let native = D::desktop_terminals_list_text(terminal, false);
        assert_eq!(
            plan(de_id, desktop, Some(&native), &all),
            P::UpToDate { filename: file.to_string() },
            "{de_id}"
        );
        let chosen = D::desktop_terminals_list_text("kitty.desktop", true);
        assert_eq!(
            plan(de_id, desktop, Some(&chosen), &all),
            P::KeepChosen { filename: file.to_string(), terminal: "kitty.desktop".into() },
            "{de_id}"
        );
    }
    // The native terminal is not installed — we write nothing.
    assert_eq!(plan("mate", "MATE", foreign, &|_| false), P::Nothing);
    // No GNOME Console — its replacement from the same set (gnome-terminal).
    let only_gt = |id: &str| id == "org.gnome.Terminal.desktop";
    assert!(matches!(
        plan("gnome", "GNOME", None, &only_gt),
        P::Write { terminal, .. } if terminal == "org.gnome.Terminal.desktop"
    ));
    // Tiling and unfamiliar desktops have no terminal of their own.
    for (de_id, desktop) in [("sway", "sway"), ("hyprland", "Hyprland"), ("niri", "niri"), ("wayfire", "wayfire")] {
        assert_eq!(plan(de_id, desktop, foreign, &all), P::Nothing, "{de_id}");
    }
    // No desktop name — the common list stays the only one.
    assert_eq!(plan("mate", "", None, &all), P::Nothing);
}

/// The person's choice beats the native one (Wehrwolfmann's decision 08.11.2026): session login
/// puts the chosen terminal into ALL native places of the desktop; without a choice — the
/// native one; chosen but not installed — the native one and a line in the log.
#[test]
fn login_puts_chosen_terminal_into_native_places_of_every_desktop() {
    use rexon_core::appdefaults::apply_de_specific_with_terminal as apply;
    let all = |_: &str| true;
    let no_helper = |_: &str| false;
    let mentions = |out: &rexon_core::appdefaults::ApplyDeSpecific, word: &str| {
        out.commands.iter().any(|c| c.iter().any(|a| a == word || a.trim_end_matches(".desktop") == word))
            || out.file_steps.iter().any(|s| format!("{s:?}").contains(word))
            || out.helpers_rc.as_deref().is_some_and(|t| t.contains(word))
    };
    for (de_id, native) in [
        ("kde", "konsole"),
        ("gnome", "kgx"),
        ("mate", "mate-terminal"),
        ("cinnamon-mint", "gnome-terminal"),
        ("xfce", "xfce4-terminal"),
        ("budgie", "xfce4-terminal"),
        ("pantheon", "io.elementary.terminal"),
        ("deepin", "deepin-terminal"),
        ("lxqt", "qterminal"),
    ] {
        let chosen = apply(de_id, Some("kitty.desktop"), &all, &all, None, &no_helper);
        assert!(mentions(&chosen, "kitty"), "{de_id}: выбор не дошёл: {chosen:?}");
        let natural = apply(de_id, None, &all, &all, None, &no_helper);
        assert!(mentions(&natural, native), "{de_id}: без выбора нет родного: {natural:?}");
        assert!(!mentions(&natural, "kitty"), "{de_id}");
        // The chosen one is not installed — the native one and an explanation.
        let missing = apply(de_id, Some("kitty.desktop"), &|id: &str| id != "kitty.desktop", &all, None, &no_helper);
        assert!(!mentions(&missing, "kitty"), "{de_id}: {missing:?}");
        assert!(mentions(&missing, native), "{de_id}: {missing:?}");
        assert!(missing.logs[0].contains("kitty"), "{de_id}: {:?}", missing.logs);
    }
    // XFCE: a chosen program without an exo helper gets its own helper,
    // and in the end it is the one in helpers.rc — as in the «Настройки» tab.
    let xfce = apply("xfce", Some("org.kde.konsole.desktop"), &all, &all, None, &no_helper);
    let last_ini = xfce.file_steps.iter().rev().find_map(|s| match s {
        D::TermStep::SetIni { key, value, .. } if key == "TerminalEmulator" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(last_ini.as_deref(), Some("custom-TerminalEmulator"), "{xfce:?}");
    assert!(xfce.file_steps.iter().any(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path.ends_with("custom-TerminalEmulator.desktop"))));
}
