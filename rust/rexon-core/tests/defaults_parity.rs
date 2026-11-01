//! Golden-сверка порта `defaults.rs` с оригиналом `rexon/defaults.py`
//! (`SystemDefaultsProtector`).
//!
//! Эталоны сняты с ЖИВОГО Python read-only (без привилегированных действий:
//! subprocess/safe_run замоканы, HOME — временный каталог) — генератор
//! прогонял чистые методы протектора на реалистичных входах и записывал
//! вход→выход в `golden/defaults_cases.json`. Покрыты: сериализация
//! snapshot/rules/pending, `_set_ini_value`, `_get_current_terminal`,
//! план `_set_terminal`, `_set_default`, `snapshot`, `restore_defaults`,
//! `get_status_summary`, `should_protect`, `resolve_choice`,
//! `_get_exec_from_desktop`, разбор/генерация `*-mimeapps.list`,
//! `protect_de_mimeapps` + таблицы-константы.

mod i18n_pin;

use std::collections::HashMap;

use rexon_core::defaults as D;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/defaults_cases.json")).unwrap()
}

// ── конвертеры JSON → типы ядра ──────────────────────────────────────────

/// Объект {k: (str|null)} → упорядоченные пары (порядок файла = порядок Python).
fn obj_pairs_opt(v: &Value) -> Vec<(String, Option<String>)> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, val)| (k.clone(), val.as_str().map(String::from)))
                .collect()
        })
        .unwrap_or_default()
}

/// Объект {k: str} → упорядоченные пары.
fn obj_pairs(v: &Value) -> Vec<(String, String)> {
    v.as_object()
        .map(|m| {
            m.iter()
                .map(|(k, val)| (k.clone(), val.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Объект {app_type: {"old":.., "new":..}} → pending-пары.
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

/// Правила из оверрайдов кейса поверх дефолтов (как make_prot в генераторе).
fn rules_from(overrides: &Value) -> D::Rules {
    let map = overrides.as_object().cloned().unwrap_or_default();
    D::Rules::defaults().updated(&map)
}

// ── сериализация snapshot / rules / pending (байт-в-байт) ─────────────────

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

// ── свой перебор терминалов + предупреждение про xdg-terminal-exec ────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pick_terminal_parity() {
    // Эталон Python — русский текст предупреждения.
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

    // Каждому кандидату — хотя бы одно имя .desktop, иначе `pick_terminal`
    // не смог бы вернуть даже канонический id.
    for (binary, ids) in D::TERMINAL_CANDIDATES {
        assert!(!ids.is_empty(), "у {binary} нет ни одного .desktop");
    }
}

// ── _set_terminal (план → сложенные runs + files) ─────────────────────────

#[test]
fn set_terminal_plan_parity() {
    const HOME: &str = "/H";
    const CFG_PREFIX: &str = "/H/.config/";
    for c in fx()["set_terminal"].as_array().unwrap() {
        // Помощники exo у системы есть (xfce4-terminal ставит их пакет
        // xfce4-settings) — сверка не зависит от состава машины.
        let plan = D::set_terminal_plan_with(
            c["terminal_desktop"].as_str().unwrap(),
            c["de"].as_str().unwrap(),
            HOME,
            &|_| true,
        );
        // Сворачиваем план в (runs, files) как это сделал бы исполнитель.
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
        // Ожидаемые runs.
        let want_runs: Vec<Vec<String>> = c["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
            .collect();
        assert_eq!(runs, want_runs, "runs for {}", c["de"]);

        // Ожидаемые файлы (относительные пути в эталоне).
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

// ── Терминал: одна таблица «окружение → родное место» ─────────────────────
//
// Намеренные расхождения с эталоном Python (эталон поправлен поимённо):
//   * `exec-arg` схемы GNOME — по терминалу: `--` у gnome-terminal (`-e` у него
//     устарел), `-x` у mate-terminal, иначе `-e`;
//   * MATE: вторым шагом `exec-arg` своей схемы (`-x` у mate-terminal);
//   * XFCE: `helpers.rc TerminalEmulator=` — раньше не писался вовсе.

/// Свернуть план в «окружение → что пишется» (runs + файлы с ключами).
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
    let common = |exec: &str, arg: &str, id: &str| {
        vec![
            format!("file:xdg-terminals.list|{id}"),
            format!("run:org.gnome.desktop.default-applications.terminal exec {exec}"),
            format!("run:org.gnome.desktop.default-applications.terminal exec-arg {arg}"),
            format!("file:environment.d/rexon-terminal.conf|TERMINAL={exec}"),
        ]
    };
    let with = |mut v: Vec<String>, extra: &[&str]| {
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    // Cinnamon — и по сырому XDG_CURRENT_DESKTOP, и по идентификатору Rexon.
    for de in ["X-Cinnamon", "cinnamon-mint", "cinnamon"] {
        assert_eq!(
            term_table(de, "org.gnome.Terminal.desktop"),
            with(
                common("gnome-terminal", "--", "org.gnome.Terminal.desktop"),
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
        with(common("alacritty", "-e", "Alacritty.desktop"), &["ini:xfce4/helpers.rc||TerminalEmulator=alacritty"])
    );
    for de in ["LXQt", "LXQt:labwc:wlroots", "lxqt"] {
        assert_eq!(
            term_table(de, "qterminal.desktop"),
            with(
                common("qterminal", "-e", "qterminal.desktop"),
                &["ini:lxqt-qtxdg.conf|[General]|TerminalEmulator=qterminal.desktop"]
            ),
            "{de}"
        );
    }
    assert_eq!(
        term_table("MATE", "mate-terminal.desktop"),
        with(
            common("mate-terminal", "-x", "mate-terminal.desktop"),
            &[
                "run:org.mate.applications-terminal exec mate-terminal",
                "run:org.mate.applications-terminal exec-arg -x",
            ]
        )
    );
    // GNOME и Budgie: родное место = общий слой, повторов нет.
    for de in ["GNOME", "Budgie:GNOME", "ubuntu:GNOME", "budgie"] {
        assert_eq!(term_table(de, "org.gnome.Console.desktop"), common("kgx", "-e", "org.gnome.Console.desktop"), "{de}");
    }
    assert_eq!(
        term_table("KDE", "org.kde.konsole.desktop"),
        with(
            common("konsole", "-e", "org.kde.konsole.desktop"),
            &[
                "ini:kdeglobals|[General]|TerminalApplication=konsole",
                "ini:kdeglobals|[General]|TerminalService=org.kde.konsole.desktop",
                "run:kdeglobals --group General --key TerminalApplication konsole",
                "run:kdeglobals --group General --key TerminalService org.kde.konsole.desktop",
            ]
        )
    );
    // Своего места нет — только общий слой, честно без «родной» записи.
    for de in ["COSMIC", "sway", "Hyprland", "niri", ""] {
        assert_eq!(term_table(de, "foot.desktop"), common("foot", "-e", "foot.desktop"), "{de}");
    }
}

/// Имя программы: явная таблица там, где бинарь не совпадает с хвостом id.
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

/// helpers.rc XFCE — файл без секций: ключ правится по месту, чужие строки
/// и комментарии остаются.
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
    // Если секции всё же есть — ключ встаёт до первого заголовка.
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

        // results (упорядочены как в Python dict).
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
        // set_call → список (как set_calls в эталоне).
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

// ── protect_de_mimeapps ───────────────────────────────────────────────────

fn desktop_name_for_test(de_id: &str) -> String {
    D::DE_DESKTOP_NAMES
        .iter()
        .find(|(k, _)| *k == de_id)
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| de_id.to_string())
}

#[test]
fn protect_de_mimeapps_parity() {
    // Тот же список «установленных» приложений, что в генераторе.
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

        // existing: de_id → content, преобразуем в path → content.
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

        // writes → сравниваем по basename с эталоном written.
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

// ── таблицы-константы ─────────────────────────────────────────────────────

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

    let names: Vec<(String, String)> = t["DE_DESKTOP_NAMES"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (a[0].as_str().unwrap().to_string(), a[1].as_str().unwrap().to_string())
        })
        .collect();
    let got_names: Vec<(String, String)> =
        D::DE_DESKTOP_NAMES.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(got_names, names, "DE_DESKTOP_NAMES");

    // DE_NATIVE_APPS: ключи и содержимое каждого DE.
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
    // и число DE совпадает.
    assert_eq!(D::DE_NATIVE_APPS.len(), want_native.len(), "DE_NATIVE_APPS count");
}

// ── язык интерфейса ─────────────────────────────────────────────────────

/// Подписи типов программ и предупреждение про xdg-terminal-exec — до
/// перевода на каталог (снимок `ca42e74^`): по-русски обязаны остаться
/// дословно прежними.
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Д7 (проверка окружений 02.10.2026): в XFCE терминал без помощника exo
/// (konsole, foot, kgx…) получал в `helpers.rc` голое имя, и
/// `exo-open --launch TerminalEmulator` открывал окно выбора. Теперь для него
/// заводится свой помощник `custom-TerminalEmulator` — как у xfce4-settings.
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
    // Каталог помощника заводится ДО записи файла.
    let mk = plan.iter().position(|s| matches!(s, D::TermStep::MakeDirs { path } if path == "/H/.local/share/xfce4/helpers"));
    let wr = plan.iter().position(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path == helper));
    assert!(mk.unwrap() < wr.unwrap());

    // Помощник системы есть — пишется его имя, своего файла нет.
    let plan = D::set_terminal_plan_with("Alacritty.desktop", "XFCE", "/H", &only_alacritty);
    assert!(!plan.iter().any(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path.contains("xfce4/helpers/"))));
    assert!(plan.contains(&D::TermStep::SetIni {
        path: "/H/.config/xfce4/helpers.rc".into(),
        section: String::new(),
        key: "TerminalEmulator".into(),
        value: "alacritty".into(),
    }));
    // Вне XFCE своих помощников не заводим.
    let plan = D::set_terminal_plan_with("org.kde.konsole.desktop", "KDE", "/H", &|_| false);
    assert!(!plan.iter().any(|s| matches!(s, D::TermStep::WriteFile { path, .. } if path.contains("xfce4"))));
}

/// Д7: файловый менеджер из «Настроек» доходит до `helpers.rc` XFCE —
/// Thunar своим помощником (строчными, как файл `thunar.desktop`), Dolphin и
/// Nemo — своим `custom-FileManager`; вне XFCE шагов нет.
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

/// Pantheon: терминал elementary пишется в схему GNOME именем бинаря
/// `io.elementary.terminal` (хвост после точки дал бы несуществующий
/// «terminal») и с `-x` — «остаток строки как команда» (`-e` у него берёт
/// одну строку на флаг). Чтение — той же схемой GNOME.
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
