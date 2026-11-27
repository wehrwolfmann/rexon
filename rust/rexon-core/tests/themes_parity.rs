//! Golden check of the `themes.rs` port against the original `rexon/themes.py`
//! (the `ThemeManager` class).
//!
//! Golden data taken from LIVE Python read-only (by a one-off generator, not part of the tree):
//! `safe_run`/`safe_check_command` mocked (no real subprocess/
//! privileged actions), HOME is a temporary directory, the built-in theme
//! catalog is the real `rexon/themes.json`. Covered: catalog merge, resolving
//! gsettings schemas + generic-wm, the apply plan for GTK/icons/cursors on various DEs
//! (+ kde-cursor fallback), writing `settings.ini`, parsing `index.theme` /
//! `kcminputrc`, font helpers, resolving the install package, "current
//! themes", KDE apply plans, history.


mod i18n_pin;
use std::collections::BTreeMap;

use rexon_core::defaults as crate_defaults;
use rexon_core::installed_scan::FsSnapshot;
use rexon_core::themes as TH;
use rexon_core::themes::{ApplyStep, Plan};
use serde_json::{json, Value};

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/themes_cases.json")).unwrap()
}

const HOME: &str = "/H";

// ── 1. Catalog merge (byte-for-byte) ─────────────────────────────────────

#[test]
fn catalog_merge_parity() {
    for c in fx()["catalog"].as_array().unwrap() {
        let merged = TH::merge_catalogs(c["builtin"].as_str(), c["user"].as_str());
        assert_eq!(
            TH::serialize_catalog(&merged),
            c["result"].as_str().unwrap(),
            "catalog merge {c:?}"
        );
    }
}

// ── 2. resolve_schemas / is_generic_wm ────────────────────────────────────

#[test]
fn resolve_schemas_parity() {
    for c in fx()["resolve"].as_array().unwrap() {
        let de = c["de"].as_str().unwrap();
        assert_eq!(TH::is_generic_wm(de), c["generic"].as_bool().unwrap(), "generic {de}");

        let schemas = TH::resolve_schemas(de);
        match c["schemas"].as_object() {
            None => assert_eq!(schemas, TH::Schemas::NonGsettings, "schemas None {de}"),
            Some(want) => {
                // The role order must match (Python dict order).
                let got: Vec<(String, Vec<String>)> = match schemas {
                    TH::Schemas::Gsettings(list) => list
                        .iter()
                        .map(|(r, s, p)| (r.to_string(), vec![s.to_string(), p.to_string()]))
                        .collect(),
                    TH::Schemas::NonGsettings => panic!("ожидались gsettings-схемы для {de}"),
                };
                let want_pairs: Vec<(String, Vec<String>)> = want
                    .iter()
                    .map(|(r, v)| {
                        let a = v.as_array().unwrap();
                        (r.clone(), vec![a[0].as_str().unwrap().to_string(), a[1].as_str().unwrap().to_string()])
                    })
                    .collect();
                assert_eq!(got, want_pairs, "schemas {de}");
            }
        }
    }
}

// ── 3. update_gtk_settings_ini ────────────────────────────────────────────

#[test]
fn update_gtk_settings_ini_parity() {
    for c in fx()["update_ini"].as_array().unwrap() {
        let got = TH::update_gtk_settings_ini(
            c["initial"].as_str(),
            c["key"].as_str().unwrap(),
            c["value"].as_str().unwrap(),
        );
        assert_eq!(got, c["result"].as_str().unwrap(), "update_ini {c:?}");
    }
}

// ── 4. apply plans: interpret the plan → (calls, files), compare with Python ─

fn cmd_success(argv: &[String], fail_mode: bool) -> bool {
    if fail_mode && argv.first().map(String::as_str) == Some("plasma-apply-cursortheme") {
        return false;
    }
    true
}

/// Run the plan like the executor: collect the list of argv calls and the resulting files
/// (relative to HOME). `fail_mode` is the failure mode (kde-cursor).
fn run_plan(plan: &Plan, fail_mode: bool) -> (Vec<Vec<String>>, BTreeMap<String, String>) {
    let mut calls: Vec<Vec<String>> = Vec::new();
    let mut vfs: BTreeMap<String, String> = BTreeMap::new();
    let mut ok = false;
    // For an IfNotOk group: the "run" decision is fixed on entering the group.
    let mut fallback_run: Option<bool> = None;

    for step in &plan.steps {
        match step {
            ApplyStep::Primary(c) => {
                fallback_run = None;
                calls.push(c.argv.clone());
                ok = cmd_success(&c.argv, fail_mode);
            }
            ApplyStep::BestEffort(c) => {
                fallback_run = None;
                calls.push(c.argv.clone());
            }
            ApplyStep::IfOk(c) => {
                fallback_run = None;
                if ok {
                    calls.push(c.argv.clone());
                }
            }
            ApplyStep::IfNotOk(c) => {
                let run = *fallback_run.get_or_insert(!ok);
                if run {
                    calls.push(c.argv.clone());
                    if cmd_success(&c.argv, fail_mode) {
                        ok = true;
                    }
                }
            }
            ApplyStep::GtkIni { path, key, value } => {
                fallback_run = None;
                let cur = vfs.get(path).cloned();
                vfs.insert(path.clone(), TH::update_gtk_settings_ini(cur.as_deref(), key, value));
            }
            ApplyStep::HyprctlCursor { theme, size } => {
                fallback_run = None;
                if size.is_none() {
                    calls.push(TH::cursor_size_get_cmd().argv);
                }
                for c in TH::hyprctl_cursor_cmds(theme, size.as_deref().unwrap_or("24")) {
                    calls.push(c.argv);
                }
            }
            ApplyStep::Live(c) => {
                fallback_run = None;
                calls.push(c.argv.clone());
            }
            ApplyStep::WriteSetsOk { path, content, .. } => {
                fallback_run = None;
                vfs.insert(path.clone(), content.clone());
                ok = true;
            }
            // The executor touches the compositor config only on disk —
            // a separate check `wm_config_executor_*`.
            ApplyStep::WmConfig(_) => fallback_run = None,
            ApplyStep::ForceOk => {
                fallback_run = None;
                ok = true;
            }
            // The Deepin stop has separate checks (`dde.rs`, `appearance_installer.rs`).
            ApplyStep::DdeTreelandGate => fallback_run = None,
            ApplyStep::WriteIfOk { path, content, .. } => {
                fallback_run = None;
                if ok {
                    vfs.insert(path.clone(), content.clone());
                }
            }
            ApplyStep::IniSetIfOk { path, section, key, value } => {
                fallback_run = None;
                if ok {
                    let cur = vfs.get(path).cloned();
                    vfs.insert(
                        path.clone(),
                        crate_defaults::set_ini_value(cur.as_deref(), section, key, value),
                    );
                }
            }
            ApplyStep::IniSetGuarded { path, section, key, value, only_if_file, keep_existing } => {
                fallback_run = None;
                if ok {
                    let cur = vfs.get(path).cloned();
                    if let Some(t) =
                        TH::ini_guarded_text(cur.as_deref(), section, key, value, *only_if_file, *keep_existing)
                    {
                        vfs.insert(path.clone(), t);
                    }
                }
            }
            ApplyStep::GtkIniUnsetIfOk { path, key } => {
                fallback_run = None;
                if ok {
                    if let Some(t) = vfs.get(path).and_then(|cur| TH::remove_gtk_settings_key(cur, key)) {
                        vfs.insert(path.clone(), t);
                    }
                }
            }
            ApplyStep::RemoveIfOk { path } => {
                fallback_run = None;
                if ok {
                    vfs.remove(path);
                }
            }
            // Theme plans do not produce these steps: they belong to «Apply» for sound, lock
            // screen wallpapers and desktop sections (`appearance_apply_plans_parity`).
            ApplyStep::Guard { .. } | ApplyStep::ReadBackSetsOk { .. } => fallback_run = None,
        }
    }

    // Paths → relative to HOME (as in the Python golden data).
    let prefix = format!("{HOME}/");
    let files = vfs
        .into_iter()
        .map(|(k, v)| (k.strip_prefix(&prefix).unwrap().to_string(), v))
        .collect();
    (calls, files)
}

#[test]
fn apply_plans_parity() {
    for c in fx()["apply"].as_array().unwrap() {
        let kind = c["kind"].as_str().unwrap();
        let theme = c["theme"].as_str().unwrap();
        let de = c["de"].as_str().unwrap();
        let fail = c["fail"].as_bool().unwrap();

        let plan = match kind {
            "gtk" => TH::apply_gtk_plan(theme, de, HOME),
            // valid/safe = true (in the generator pre-flight is mocked to allow).
            "icons" => TH::apply_icon_plan(theme, de, HOME, true).expect("icon plan"),
            "cursor" => TH::apply_cursor_plan(theme, de, HOME, true).expect("cursor plan"),
            other => panic!("неизвестный kind {other}"),
        };

        let (calls, files) = run_plan(&plan, fail);

        let want_calls: Vec<Vec<String>> = c["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
            .collect();
        assert_eq!(calls, want_calls, "apply calls {kind}/{de} fail={fail}");

        let mut want_files: BTreeMap<String, String> = c["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        // Intentional divergence from Python: on LXQt the GTK theme is now also written
        // to settings.ini (as lxqt-config-appearance does) — without xsettingsd GTK
        // reads exactly the file, and gsettings was left without a reader.
        // Intentional divergence from Python (30.09): outside KDE the cursor also reaches
        // programs that read XCURSOR_* from the environment (Qt without a platform
        // theme, Electron, XWayland) — the environment.d file.
        if kind == "cursor" && de != "kde" {
            want_files.insert(
                ".config/environment.d/rexon-cursor.conf".to_string(),
                format!("XCURSOR_THEME={theme}\n"),
            );
        }
        // Intentional divergence from Python (24.10): outside KDE icons also reach
        // KDE programs (KIconThemes reads `kdeglobals [Icons] Theme`).
        if kind == "icons" && de != "kde" {
            want_files.insert(".config/kdeglobals".to_string(), format!("\n[Icons]\nTheme={theme}\n"));
        }
        if de == "lxqt" && kind == "gtk" {
            for v in ["gtk-3.0", "gtk-4.0"] {
                want_files.insert(
                    format!(".config/{v}/settings.ini"),
                    format!("[Settings]\ngtk-theme-name={theme}\n"),
                );
            }
        }
        assert_eq!(files, want_files, "apply files {kind}/{de} fail={fail}");
    }

    // Pre-flight refusal: an invalid icon theme / unsafe cursor → no plan.
    assert!(TH::apply_icon_plan("Cursorish", "gnome", HOME, false).is_none());
    assert!(TH::apply_cursor_plan("Adwaita", "gnome", HOME, false).is_none());
}

// ── 5. is_valid_icon_theme (parsing index.theme) ───────────────────────────

fn fs_with_index(content: Option<&str>) -> FsSnapshot {
    let mut files = serde_json::Map::new();
    if let Some(text) = content {
        files.insert("/i/TestTheme/index.theme".to_string(), Value::from(text));
    }
    FsSnapshot::from_json(&json!({"dirs": {}, "files": files}))
}

#[test]
fn is_valid_icon_theme_parity() {
    let dirs = vec!["/i".to_string()];
    for c in fx()["is_valid_icon"].as_array().unwrap() {
        let fs = fs_with_index(c["index"].as_str());
        let got = TH::is_valid_icon_theme(&fs, "TestTheme", &dirs);
        assert_eq!(got, c["result"].as_bool().unwrap(), "is_valid_icon {}", c["label"]);
    }
    // empty name
    let fs = fs_with_index(None);
    assert_eq!(
        TH::is_valid_icon_theme(&fs, "", &dirs),
        fx()["is_valid_icon_emptyname"].as_bool().unwrap()
    );
}

// ── 6. kde_kcminput_cursor / kde_font_to_display / system_font_fallback ────

#[test]
fn kde_kcminput_cursor_parity() {
    for c in fx()["kcminput"].as_array().unwrap() {
        let got = TH::kde_kcminput_cursor(c["content"].as_str());
        assert_eq!(got, c["result"].as_str().unwrap(), "kcminput {}", c["label"]);
    }
}

#[test]
fn kde_font_to_display_parity() {
    for c in fx()["kde_font"].as_array().unwrap() {
        let got = TH::kde_font_to_display(c["raw"].as_str().unwrap());
        assert_eq!(got, c["result"].as_str().unwrap(), "kde_font {c:?}");
    }
}

#[test]
fn system_font_fallback_parity() {
    for c in fx()["font_fallback"].as_array().unwrap() {
        let got = TH::system_font_fallback(c["gtk4"].as_str(), c["gtk3"].as_str(), c["fc"].as_str());
        assert_eq!(got, c["result"].as_str().unwrap(), "font_fallback {}", c["label"]);
    }
}

// ── 7. resolve_install ────────────────────────────────────────────────────

#[test]
fn resolve_install_parity() {
    for c in fx()["resolve_install"].as_array().unwrap() {
        let (pkg, aur) = TH::resolve_install(&c["meta"], c["family"].as_str().unwrap());
        let want = c["result"].as_array().unwrap();
        assert_eq!(pkg, want[0].as_str().unwrap(), "pkg {c:?}");
        assert_eq!(aur, want[1].as_str().unwrap(), "aur {c:?}");
    }
}

// ── 8. get_current_themes ─────────────────────────────────────────────────

fn opt<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

#[test]
fn get_current_themes_parity() {
    for c in fx()["current_themes"].as_array().unwrap() {
        let de = c["de"].as_str().unwrap();
        let gs = &c["gs"];
        let xf = &c["xf"];
        let kde = &c["kde"];
        let font_fb = c["font_fb"].as_str().unwrap();

        let src = TH::CurrentSources {
            gs_gtk: opt(gs, "org.gnome.desktop.interface|gtk-theme"),
            gs_icons: opt(gs, "org.gnome.desktop.interface|icon-theme"),
            gs_cursor: opt(gs, "org.gnome.desktop.interface|cursor-theme"),
            gs_font: opt(gs, "org.gnome.desktop.interface|font-name"),
            xf_gtk: opt(xf, "/Net/ThemeName"),
            xf_icons: opt(xf, "/Net/IconThemeName"),
            xf_cursor: opt(xf, "/Gtk/CursorThemeName"),
            xf_font: opt(xf, "/Gtk/FontName"),
            kde_widget_style: opt(kde, "General|widgetStyle"),
            kde_icons: opt(kde, "Icons|Theme"),
            kde_cursor_mouse: opt(kde, "Mouse|cursorTheme"),
            kde_kcminput_cursor: c["kcmin"].as_str().unwrap(),
            xcursor_env: c["xenv"].as_str().unwrap_or(""),
            kde_font: opt(kde, "General|font"),
            font_fallback: font_fb,
        };
        let got = TH::get_current_themes(de, &src);
        assert_eq!(Value::Object(got), c["result"], "current_themes {de} {c:?}");
    }
}

// ── 9. KDE apply plans + get_current_kde_cmds ─────────────────────────────

fn flat_cmd(c: &TH::Cmd) -> Value {
    json!({"argv": c.argv, "timeout": c.timeout})
}

#[test]
fn kde_apply_parity() {
    let f = fx();
    let ka = &f["kde_apply"];

    let single = |plan: &Plan| -> Value {
        assert_eq!(plan.steps.len(), 1);
        match &plan.steps[0] {
            ApplyStep::Primary(c) => flat_cmd(c),
            other => panic!("ожидался Primary, а не {other:?}"),
        }
    };

    assert_eq!(json!([single(&TH::apply_kde_global_plan("Breeze"))]), ka["global"], "kde global");
    assert_eq!(json!([single(&TH::apply_kde_plasma_plan("default"))]), ka["plasma"], "kde plasma");
    assert_eq!(json!([single(&TH::apply_kde_color_plan("BreezeDark"))]), ka["colors"], "kde colors");

    // kvantum on KDE: set → widget_style → notify (the golden data was taken from the KDE path,
    // which stayed untouched; outside KDE the second step is different — see the unit
    // tests kvantum_* in themes.rs).
    let kv = TH::apply_kvantum_plan(
        "KvArc",
        &TH::KvantumEnv {
            de_id: "kde",
            home: "/home/t",
            xdg_config_home: "",
            platform_theme: "",
            icon_theme: String::new(),
            qt_desktop_installed: false,
        },
    );
    let got_kv: Value = kv
        .steps
        .iter()
        .map(|s| match s {
            ApplyStep::Primary(c) | ApplyStep::IfOk(c) => flat_cmd(c),
            other => panic!("на KDE ожидались только команды, а не {other:?}"),
        })
        .collect::<Vec<_>>()
        .into();
    assert_eq!(got_kv, ka["kvantum"], "kvantum");

    // get_current_kde_cmds.
    let want_current: Vec<Vec<String>> = ka["current"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .collect();
    let got_current: Vec<Vec<String>> = TH::get_current_kde_cmds().iter().map(|c| c.argv.clone()).collect();
    assert_eq!(got_current, want_current, "current kde cmds");
}

// ── 9-bis. Installed theme scanners (FS snapshot) ──────────────────────────

fn scanners_fx() -> Value {
    serde_json::from_str(include_str!("golden/themes_scanners.json")).unwrap()
}

fn want_list(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn list_scanners_parity() {
    let f = scanners_fx();
    let fs = FsSnapshot::from_json(&f["fs"]);
    let home = f["home"].as_str().unwrap();
    let e = &f["expect"];

    assert_eq!(TH::list_gtk_themes(&fs, home), want_list(&e["gtk"]), "list_gtk_themes");
    assert_eq!(TH::list_icon_themes(&fs, home), want_list(&e["icons"]), "list_icon_themes");
    assert_eq!(TH::list_cursor_themes(&fs, home), want_list(&e["cursors"]), "list_cursor_themes");
    assert_eq!(TH::list_kde_global_themes(&fs, home), want_list(&e["kde_global"]), "list_kde_global");
    assert_eq!(TH::list_kde_plasma_styles(&fs, home), want_list(&e["kde_plasma"]), "list_kde_plasma");
    assert_eq!(TH::list_kde_color_schemes(&fs, home), want_list(&e["kde_colors"]), "list_kde_colors");

    // kvantum: dir_files — only files inside the theme directory.
    let dir_files = |base: &str| -> Vec<String> {
        fs.scandir(base)
            .iter()
            .filter(|n| fs.is_file(&format!("{base}/{n}")))
            .cloned()
            .collect()
    };
    assert_eq!(
        TH::list_kvantum_themes(&fs, home, dir_files),
        want_list(&e["kvantum"]),
        "list_kvantum_themes"
    );
}

// ── 10. history (deterministic logic) ────────────────────────────────

#[test]
fn history_parity() {
    let f = fx();
    let ts = f["history_ts"].as_str().unwrap();
    let e = TH::history_entry("gtk", "Dracula", "kde", ts);
    assert_eq!(
        e,
        json!({"kind": "gtk", "theme": "Dracula", "de": "kde", "ts": ts})
    );

    // push: insert at the front.
    let h0: Vec<Value> = Vec::new();
    let h1 = TH::push_history(&h0, e.clone());
    assert_eq!(h1, vec![e.clone()]);
    let e2 = TH::history_entry("icons", "Papirus", "kde", ts);
    let h2 = TH::push_history(&h1, e2.clone());
    assert_eq!(h2, vec![e2.clone(), e.clone()]);

    // truncate to 50.
    let mut big: Vec<Value> = (0..60).map(|i| json!({"n": i})).collect();
    let pushed = TH::push_history(&big, e.clone());
    assert_eq!(pushed.len(), 50);
    assert_eq!(pushed[0], e);
    assert_eq!(pushed[1], json!({"n": 0}));

    // get_recent.
    big.truncate(5);
    assert_eq!(TH::get_recent(&big, 3).len(), 3);
    assert_eq!(TH::get_recent(&big, 100).len(), 5);

    // serialize — a valid JSON array (ensure_ascii=False, indent=2).
    let s = TH::serialize_history(&h2);
    assert!(s.starts_with("[\n"));
    assert!(s.contains("\"theme\": \"Papirus\""));
}

// ══════════════════════════════════════════════════════════════════════════
//  Second wave of the port: theme archive, catalog texts, reading kdeglobals
// ══════════════════════════════════════════════════════════════════════════
//
// The `pure_v2` golden data was taken from live Python on THIS machine's data: the real
// `~/.config/kdeglobals` (values via `configparser(strict=False)`, as in the
// `_kde_get` fallback) and the real theme directory names from `~/.themes` and
// `/usr/share/themes` (with spaces and Cyrillic — we check the sorting).

fn strs2(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn texts_match_python() {
    // The golden data was taken with the Russian interface — the check runs on the Russian catalog.
    let _lang = i18n_pin::ru();
    let t = &fx()["pure_v2"]["texts"];
    assert_eq!(json!(TH::msg_archive_no_dirs()), t["ARCHIVE_NO_DIRS"]);
    assert_eq!(
        json!(TH::msg_extract_failed_prefix()),
        t["EXTRACT_FAILED_PREFIX"]
    );
    assert_eq!(
        json!(TH::msg_theme_not_in_catalog_prefix()),
        t["THEME_NOT_IN_CATALOG_PREFIX"]
    );
    assert_eq!(
        json!(TH::msg_installed_from_repo_prefix()),
        t["INSTALLED_FROM_REPO_PREFIX"]
    );
    assert_eq!(
        json!(TH::msg_installed_from_aur_prefix()),
        t["INSTALLED_FROM_AUR_PREFIX"]
    );
    assert_eq!(json!(TH::msg_no_package_prefix()), t["NO_PACKAGE_PREFIX"]);
    assert_eq!(
        json!(TH::msg_install_failed_prefix()),
        t["INSTALL_FAILED_PREFIX"]
    );
}

#[test]
fn archive_install_matches_python() {
    let p = &fx()["pure_v2"];
    let home = p["home"].as_str().unwrap();
    for c in p["archive_dest"].as_array().unwrap() {
        let d = TH::archive_install_dest(
            c["kind"].as_str().unwrap(),
            c["name"].as_str().unwrap(),
            home,
            None,
        );
        assert_eq!(
            vec![d.base, d.dest],
            strs2(&c["want"]),
            "kind={} name={}",
            c["kind"],
            c["name"]
        );
    }
    for c in p["pick_archive_dir"].as_array().unwrap() {
        let got = TH::pick_archive_dir(&strs2(&c["names"]));
        assert_eq!(json!(got), c["want"], "names={}", c["names"]);
    }
}

#[test]
fn kdeglobals_read_matches_python() {
    let p = &fx()["pure_v2"];
    for c in p["kde_read_cmd"].as_array().unwrap() {
        let cmd = TH::kde_read_cmd(c["section"].as_str().unwrap(), c["key"].as_str().unwrap());
        assert_eq!(cmd.argv, strs2(&c["argv"]));
        assert_eq!(cmd.timeout, None);
    }
    let g = &p["kdeglobals"];
    let content = g["content"].as_str().unwrap();
    for c in g["cases"].as_array().unwrap() {
        let got = TH::kdeglobals_value(
            content,
            c["section"].as_str().unwrap(),
            c["key"].as_str().unwrap(),
        );
        assert_eq!(
            json!(got),
            c["want"],
            "[{}] {}",
            c["section"],
            c["key"]
        );
    }
}

// ── 14. Kvantum outside KDE: the theme installs everywhere, the Qt style goes its own way ──
//
// Python has no golden data for these branches: until now the second step was done
// only for KDE, and in other desktops the section was simply hidden. We check
// not byte parity but promises: which mechanism goes where, and that the person is not left
// without a result.

fn kv_plan(de: &str, platform_theme: &str) -> Plan {
    TH::apply_kvantum_plan(
        "KvArc",
        &TH::KvantumEnv {
            de_id: de,
            home: "/home/t",
            xdg_config_home: "",
            platform_theme,
            icon_theme: String::new(),
            qt_desktop_installed: false,
        },
    )
}

/// The first step is the same everywhere: choosing the Kvantum theme itself.
#[test]
fn kvantum_sets_theme_in_every_desktop() {
    for de in ["kde", "lxqt", "gnome", "xfce", "sway", "", "какой-то-новый-wm"] {
        let p = kv_plan(de, "");
        match &p.steps[0] {
            ApplyStep::Primary(c) => {
                assert_eq!(c.argv, ["kvantummanager", "--set", "KvArc"], "{de}")
            }
            other => panic!("{de}: первым шагом ожидался выбор темы, а не {other:?}"),
        }
        assert!(p.steps.len() >= 2, "{de}: стиль Qt не переключается");
        assert_eq!(p.history_kind, "kvantum");
    }
}

/// KDE and LXQt edit THEIR OWN settings; there must be no environment variable there
/// — it would override the regular style choice.
#[test]
fn kvantum_uses_native_switch_on_kde_and_lxqt() {
    let kde = kv_plan("kde", "qt6ct");
    assert!(kde
        .steps
        .iter()
        .all(|s| matches!(s, ApplyStep::Primary(_) | ApplyStep::IfOk(_))));
    assert!(kde.steps.iter().any(|s| matches!(
        s, ApplyStep::IfOk(c) if c.argv.contains(&"widgetStyle".to_string())
    )));

    let lxqt = kv_plan("lxqt", "qt6ct");
    match &lxqt.steps[1] {
        ApplyStep::IniSetIfOk { path, section, key, value } => {
            assert_eq!(path, "/home/t/.config/lxqt/lxqt.conf");
            assert_eq!(
                (section.as_str(), key.as_str(), value.as_str()),
                ("[Qt]", "style", "kvantum")
            );
        }
        other => panic!("LXQt правит свой lxqt.conf, а не {other:?}"),
    }
    for p in [&kde, &lxqt] {
        assert!(
            !p.steps.iter().any(|s| matches!(
                s, ApplyStep::WriteIfOk { path, .. } if path.contains("environment.d")
            )),
            "переменная окружения не пишется там, где стиль выбирается штатно"
        );
    }
}

/// The full path: qt6ct is present — the style is written to its config, and the variable
/// makes it the platform theme (then fonts and icons follow too).
#[test]
fn kvantum_full_path_uses_qt6ct() {
    let p = kv_plan("gnome", "qt6ct");
    assert!(p.steps.iter().any(|s| matches!(
        s, ApplyStep::IniSetIfOk { path, section, key, value }
        if path == "/home/t/.config/qt6ct/qt6ct.conf"
            && section == "[Appearance]" && key == "style" && value == "kvantum"
    )));
    let env = p
        .steps
        .iter()
        .find_map(|s| match s {
            ApplyStep::WriteIfOk { path, content, .. } => Some((path, content)),
            _ => None,
        })
        .expect("файл environment.d");
    assert_eq!(env.0, "/home/t/.config/environment.d/rexon-qt-style.conf");
    assert_eq!(env.1, "QT_QPA_PLATFORMTHEME=qt6ct\n");
    // We do NOT write QT_STYLE_OVERRIDE here: it would override the style choice in qt6ct itself.
    assert!(!env.1.contains("QT_STYLE_OVERRIDE"));
}

/// The fallback path is a full one: no qt6ct (declined, password cancelled, the package
/// is not in the repositories) — the theme is applied anyway.
#[test]
fn kvantum_falls_back_to_env_variable() {
    for de in ["gnome", "xfce", "sway", "cosmic", "budgie", ""] {
        let p = kv_plan(de, "");
        let env = p
            .steps
            .iter()
            .find_map(|s| match s {
                ApplyStep::WriteIfOk { dir, path, content } => Some((dir, path, content)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{de}: без qt6ct нет запасного пути"));
        assert_eq!(env.0, "/home/t/.config/environment.d");
        assert_eq!(env.1, "/home/t/.config/environment.d/rexon-qt-style.conf");
        assert_eq!(env.2, "QT_STYLE_OVERRIDE=kvantum\n");
        // Without qt6ct other configs are not touched at all.
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::IniSetIfOk { .. })));
    }
}

/// `$XDG_CONFIG_HOME` is respected — otherwise the file would land outside the session.
#[test]
fn kvantum_respects_xdg_config_home() {
    let p = TH::apply_kvantum_plan(
        "KvArc",
        &TH::KvantumEnv {
            de_id: "gnome",
            home: "/home/t",
            xdg_config_home: "/home/t/cfg",
            platform_theme: "",
            icon_theme: String::new(),
            qt_desktop_installed: false,
        },
    );
    assert!(p.steps.iter().any(|s| matches!(
        s, ApplyStep::WriteIfOk { path, .. }
        if path == "/home/t/cfg/environment.d/rexon-qt-style.conf"
    )));
}

/// The package name is taken by distribution family, not "qt6ct everywhere", and
/// is taken from the SHARED package table, not from a second copy of our own.
#[test]
fn qt_style_helper_package_is_per_family() {
    let pkg = |f: &str| TH::qt_style_helper_pkg(f);
    for family in ["arch", "debian", "fedora", "suse", "alpine", "void", "solus", "alt"] {
        assert_eq!(pkg(family).as_deref(), Some("qt6ct"), "{family}");
    }
    // Gentoo keeps names with the category.
    assert_eq!(pkg("gentoo").as_deref(), Some("gui-apps/qt6ct"));
    // Nothing to install with — no extra install, the fallback path will work.
    assert_eq!(pkg("nix"), None);
    assert_eq!(pkg("clear"), None);
    // An unknown family — try the most common name: a miss is harmless,
    // the theme will be applied via the fallback path.
    assert_eq!(pkg("какое-то-новое").as_deref(), Some("qt6ct"));
    // The same table as for the other tools.
    assert_eq!(
        rexon_core::utils::tool_package_name(TH::QT_STYLE_TOOL, "gentoo"),
        "gui-apps/qt6ct"
    );
}

// ── 14-bis. «Application Style» — the same fork as Kvantum ─────────────
//
// The table "desktop → what gets written": KDE — kdeglobals + notification, LXQt —
// lxqt.conf [Qt] style, the rest — qt6ct.conf + environment.d (or
// QT_STYLE_OVERRIDE without qt6ct). The section used to write kdeglobals everywhere.

fn ws_plan(de: &str, platform_theme: &str) -> Plan {
    TH::apply_qt_style_plan(
        "Fusion",
        &TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme, icon_theme: String::new(), qt_desktop_installed: false },
    )
}

/// Summary table: which file and which key gets the style in each desktop.
#[test]
fn widget_style_table_by_desktop() {
    // (desktop, qt6ct, expected records "path|section|key=value" or argv)
    let describe = |p: &Plan| -> Vec<String> {
        p.steps
            .iter()
            .map(|s| match s {
                ApplyStep::Primary(c) => format!("primary:{}", c.argv.join(" ")),
                ApplyStep::IfOk(c) => format!("ifok:{}", c.argv[0]),
                ApplyStep::ForceOk => "forceok".to_string(),
                ApplyStep::IniSetIfOk { path, section, key, value } => {
                    format!("ini:{path}|{section}|{key}={value}")
                }
                ApplyStep::WriteIfOk { path, content, .. } => {
                    format!("write:{path}|{}", content.trim_end())
                }
                ApplyStep::RemoveIfOk { path } => format!("remove:{path}"),
                other => panic!("неожиданный шаг {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        describe(&ws_plan("kde", "qt6ct")),
        [
            "primary:kwriteconfig6 --file kdeglobals --group KDE --key widgetStyle Fusion",
            "ifok:dbus-send",
        ]
    );
    assert_eq!(
        describe(&ws_plan("lxqt", "qt6ct")),
        ["forceok", "ini:/home/t/.config/lxqt/lxqt.conf|[Qt]|style=Fusion"]
    );
    for de in [
        "gnome", "cinnamon-mint", "cinnamon", "xfce", "mate", "budgie", "cosmic", "sway",
        "hyprland", "niri", "", "какой-то-новый-wm",
    ] {
        assert_eq!(
            describe(&ws_plan(de, "qt6ct")),
            [
                "forceok",
                "ini:/home/t/.config/qt6ct/qt6ct.conf|[Appearance]|style=Fusion",
                "write:/home/t/.config/environment.d/rexon-qt-style.conf|QT_QPA_PLATFORMTHEME=qt6ct",
                "remove:/home/t/.config/rexon/session-env/rexon-qt-style.conf",
            ],
            "{de} с qt6ct"
        );
        assert_eq!(
            describe(&ws_plan(de, "")),
            [
                "forceok",
                "write:/home/t/.config/environment.d/rexon-qt-style.conf|QT_STYLE_OVERRIDE=Fusion",
                "remove:/home/t/.config/rexon/session-env/rexon-qt-style.conf",
            ],
            "{de} без qt6ct"
        );
        // kdeglobals is no longer written outside KDE.
        assert!(!describe(&ws_plan(de, "")).iter().any(|s| s.contains("kdeglobals")), "{de}");
    }
}

/// Kvantum and «Application Style» are one fork: the tail of the Kvantum plan matches
/// the style plan "kvantum", differing only in coming after its own Primary.
#[test]
fn widget_style_and_kvantum_share_one_switch() {
    for de in ["kde", "lxqt", "gnome", "sway"] {
        for tool in ["qt6ct", ""] {
            let kv = kv_plan(de, tool);
            let ws = TH::apply_qt_style_plan(
                "kvantum",
                &TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme: tool, icon_theme: String::new(), qt_desktop_installed: false },
            );
            let norm = |s: &ApplyStep| match s {
                ApplyStep::Primary(c) => ApplyStep::IfOk(c.clone()),
                o => o.clone(),
            };
            let ws_tail: Vec<ApplyStep> = ws
                .steps
                .iter()
                .filter(|s| !matches!(s, ApplyStep::ForceOk))
                .map(norm)
                .collect();
            assert_eq!(kv.steps[1..].to_vec(), ws_tail, "{de}/{tool}");
        }
    }
    assert_eq!(ws_plan("gnome", "").history_kind, "widget-style");
}

// ── 14-ter. LXQt: native keys for the GTK theme, icons and cursor ─────────────────
//
// The keys were checked against the strings of the installed LXQt 2.x binaries:
// lxqt-config-appearance — `icon_theme`, `$XDG_CONFIG_HOME/gtk-%1/settings.ini`,
// `Mouse/cursor_theme`; lxqt-session — `Mouse`/`cursor_theme`.

#[test]
fn lxqt_gets_native_theme_keys() {
    let (_, gtk) = run_plan(&TH::apply_gtk_plan("Dracula", "lxqt", HOME), false);
    assert_eq!(gtk[".config/gtk-3.0/settings.ini"], "[Settings]\ngtk-theme-name=Dracula\n");
    assert_eq!(gtk[".config/gtk-4.0/settings.ini"], "[Settings]\ngtk-theme-name=Dracula\n");

    // Icons: gsettings (for GTK) + our own lxqt.conf; ForceOk before it —
    // a failed gsettings (no dconf) must not take the native key away from LXQt.
    let icons = TH::apply_icon_plan("Papirus", "lxqt", HOME, true).unwrap();
    let force_at = icons.steps.iter().position(|s| matches!(s, ApplyStep::ForceOk)).unwrap();
    let ini_at = icons.steps.iter().position(|s| matches!(s, ApplyStep::IniSetIfOk { .. })).unwrap();
    assert!(force_at < ini_at);
    let (calls, files) = run_plan(&icons, true);
    assert_eq!(calls[0][..3], ["gsettings", "set", "org.gnome.desktop.interface"]);
    assert_eq!(files[".config/lxqt/lxqt.conf"], "\n[General]\nicon_theme=Papirus\n");

    let cursor = TH::apply_cursor_plan("Bibata", "lxqt", HOME, true).unwrap();
    let (_, files) = run_plan(&cursor, true);
    assert_eq!(files[".config/lxqt/session.conf"], "\n[Mouse]\ncursor_theme=Bibata\n");
    assert_eq!(files[".icons/default/index.theme"], "[Icon Theme]\nInherits=Bibata\n");

    // Other keys in lxqt.conf are kept, the value is edited in its own section.
    let before = "[General]\ntheme=Clearlooks\nicon_theme=breeze\n\n[Qt]\nstyle=Fusion\n";
    assert_eq!(
        crate_defaults::set_ini_value(Some(before), "[General]", "icon_theme", "Papirus"),
        "[General]\ntheme=Clearlooks\nicon_theme=Papirus\n\n[Qt]\nstyle=Fusion\n"
    );

    // Other desktops do not get the LXQt keys.
    for de in ["gnome", "kde", "xfce", "sway", "cinnamon-mint"] {
        let p = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        let c = TH::apply_cursor_plan("Bibata", de, HOME, true).unwrap();
        for step in p.steps.iter().chain(c.steps.iter()) {
            if let ApplyStep::IniSetIfOk { path, .. } = step {
                panic!("{de}: лишняя запись {path}");
            }
        }
    }
}

/// COSMIC: icons also go into the native libcosmic key (panel, cosmic-files),
/// as a quoted RON string — the way `cosmic-config` writes it; if the write succeeded,
/// applying succeeded, even if gsettings did not answer. Other desktops
/// do not get this key.
#[test]
fn cosmic_icons_write_cosmic_tk_key() {
    let icons = TH::apply_icon_plan("Papirus-Dark", "cosmic", HOME, true).unwrap();
    let (calls, files) = run_plan(&icons, true);
    assert_eq!(calls[0][..3], ["gsettings", "set", "org.gnome.desktop.interface"]);
    assert_eq!(files[".config/cosmic/com.system76.CosmicTk/v1/icon_theme"], "\"Papirus-Dark\"");
    assert!(icons.steps.iter().any(|s| matches!(s, ApplyStep::WriteSetsOk { dir, .. }
        if dir == "/H/.config/cosmic/com.system76.CosmicTk/v1")));
    let xdg = TH::apply_icon_plan_xdg("A\\B\"C", "cosmic", HOME, Some("/X"), true).unwrap();
    let written: Vec<(&str, &str)> = xdg
        .steps
        .iter()
        .filter_map(|s| match s {
            ApplyStep::WriteSetsOk { path, content, .. } => Some((path.as_str(), content.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(written, vec![("/X/cosmic/com.system76.CosmicTk/v1/icon_theme", "\"A\\\\B\\\"C\"")]);
    assert!(TH::cosmic_ron_string("a\nb").is_none());
    for de in ["gnome", "kde", "xfce", "sway", "lxqt", "cinnamon-mint"] {
        let p = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::WriteSetsOk { path, .. } if path.contains("cosmic"))), "{de}");
    }
}

/// There is no point asking about the package where the Qt style is chosen the regular way.
#[test]
fn qt_style_way_matches_desktop() {
    use rexon_core::themes::QtStyleWay;
    assert_eq!(TH::qt_style_way("kde"), QtStyleWay::Kdeglobals);
    assert_eq!(TH::qt_style_way("lxqt"), QtStyleWay::LxqtConf);
    for de in [
        "gnome", "cinnamon-mint", "xfce", "mate", "budgie", "cosmic", "sway",
        "hyprland", "niri", "", "чего-то-новое",
    ] {
        assert_eq!(TH::qt_style_way(de), QtStyleWay::PlatformTheme, "{de}");
    }
}

/// Deepin: DTK programs are drawn with the Chameleon style, and a session-wide Qt style
/// variable would take it away. Neither `QT_QPA_PLATFORMTHEME` nor
/// `QT_STYLE_OVERRIDE` is written, and an empty plan honestly does not give "success";
/// the section is hidden in Deepin with its own reason.
#[test]
fn deepin_keeps_chameleon_no_session_qt_variable() {
    use rexon_core::themes::QtStyleWay;
    assert_eq!(TH::qt_style_way("deepin"), QtStyleWay::Deepin);
    let env = TH::KvantumEnv {
        de_id: "deepin",
        home: "/home/t",
        xdg_config_home: "",
        platform_theme: "qt6ct",
        icon_theme: "Papirus".into(),
        qt_desktop_installed: false,
    };
    for plan in [TH::apply_kvantum_plan("KvArc", &env), TH::apply_qt_style_plan("Fusion", &env)] {
        assert!(plan.steps.is_empty(), "{:?}", plan.steps);
    }
    assert!(!rexon_core::categories::category_available("kvantum", "deepin"));
    assert!(!rexon_core::categories::category_available("widgetstyle", "deepin"));
    assert!(rexon_core::categories::category_available("kvantum", "gnome"));
    let _lang = i18n_pin::en();
    let why = rexon_core::de_customization::unavailable_note(
        "theme-kvantum",
        rexon_core::de_customization::De::Deepin,
        "Deepin",
    )
    .unwrap();
    assert!(why.contains("Chameleon"), "{why}");
    // The "after logging in again" caveat is not shown in Deepin — there is no section there.
    assert!(rexon_core::de_customization::condition_note_id("theme-kvantum", "deepin").is_none());
}

/// The same messages in the source language.
#[test]
fn texts_in_english() {
    let _lang = i18n_pin::en();
    assert_eq!(TH::msg_archive_no_dirs(), "The archive contains no folders");
    assert_eq!(TH::msg_extract_failed_prefix(), "Unpacking error: ");
    assert_eq!(TH::msg_install_failed_prefix(), "Couldn't install. Try it manually: ");
}

// ── 15. Cursor in all desktops: theme + size (30.09) ──────────────────
//
// The native keys were checked against primary sources (see the section header in themes.rs):
// plasma-apply-cursortheme --help (`--size`), sway-input(5) (`seat … xcursor_theme
// <theme> [<size>]`), the niri wiki (`cursor { xcursor-theme; xcursor-size }`,
// `include` since 25.11), the Hyprland 0.56.2 wiki and sources (`hl.env`, `env =`,
// hyprland.lua before hyprland.conf), xfsettingsd strings (`/Gtk/CursorThemeSize`).

fn sized(de: &str) -> Plan {
    TH::apply_cursor_plan_sized("Bibata", Some(32), de, HOME, None, true).unwrap()
}

/// What each desktop gets when a theme and a size are chosen.
#[test]
fn cursor_size_reaches_every_desktop() {
    let cases: &[(&str, &[&str])] = &[
        ("kde", &[
            "plasma-apply-cursortheme --size 32 Bibata",
            "kwriteconfig6 --file kcminputrc --group Mouse --key cursorSize 32",
        ]),
        ("gnome", &[
            "gsettings set org.gnome.desktop.interface cursor-theme Bibata",
            "gsettings set org.gnome.desktop.interface cursor-size 32",
        ]),
        ("budgie", &["gsettings set org.gnome.desktop.interface cursor-size 32"]),
        ("cosmic", &["gsettings set org.gnome.desktop.interface cursor-size 32"]),
        ("cinnamon", &[
            "gsettings set org.cinnamon.desktop.interface cursor-theme Bibata",
            "gsettings set org.cinnamon.desktop.interface cursor-size 32",
        ]),
        ("mate", &[
            "gsettings set org.mate.peripherals-mouse cursor-theme Bibata",
            "gsettings set org.mate.peripherals-mouse cursor-size 32",
        ]),
        ("xfce", &[
            "xfconf-query -c xsettings -p /Gtk/CursorThemeName -s Bibata",
            "xfconf-query -c xsettings -p /Gtk/CursorThemeSize -n -t int -s 32",
        ]),
        ("sway", &["swaymsg seat * xcursor_theme Bibata 32"]),
        ("hyprland", &["hyprctl setcursor Bibata 32", "hyprctl keyword env XCURSOR_THEME,Bibata"]),
    ];
    for (de, want) in cases {
        let (calls, files) = run_plan(&sized(de), false);
        let calls: Vec<String> = calls.iter().map(|c| c.join(" ")).collect();
        for w in *want {
            assert!(calls.iter().any(|c| c == w), "{de}: нет «{w}» в {calls:?}");
        }
        for v in ["gtk-3.0", "gtk-4.0"] {
            assert_eq!(
                files[&format!(".config/{v}/settings.ini")],
                "[Settings]\ngtk-cursor-theme-name=Bibata\ngtk-cursor-theme-size=32\n",
                "{de}"
            );
        }
        assert_eq!(files[".icons/default/index.theme"], "[Icon Theme]\nInherits=Bibata\n", "{de}");
        let env = files.get(".config/environment.d/rexon-cursor.conf");
        if *de == "kde" {
            // Plasma sets XCURSOR_* from kcminputrc itself at login.
            assert!(env.is_none(), "kde: лишний environment.d");
        } else {
            assert_eq!(env.map(String::as_str), Some("XCURSOR_THEME=Bibata\nXCURSOR_SIZE=32\n"), "{de}");
        }
    }
    let (_, lxqt) = run_plan(&sized("lxqt"), false);
    assert_eq!(lxqt[".config/lxqt/session.conf"], "\n[Mouse]\ncursor_theme=Bibata\ncursor_size=32\n");
    // Without a size the size is touched nowhere.
    let (calls, files) = run_plan(&TH::apply_cursor_plan("Bibata", "gnome", HOME, true).unwrap(), false);
    assert!(!calls.iter().any(|c| c.contains(&"cursor-size".to_string())));
    assert!(!files[".config/gtk-3.0/settings.ini"].contains("size"));
}

/// For tiling desktops "applied" is no longer unconditional: `ok` comes from writing our
/// environment file, and the compositor config is edited in a separate step.
#[test]
fn tiling_cursor_status_is_true() {
    for de in ["sway", "niri", "hyprland", "river", "labwc"] {
        let p = sized(de);
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::ForceOk)), "{de}: ForceOk");
        assert!(
            !p.steps.iter().any(|s| matches!(s, ApplyStep::Primary(_))),
            "{de}: gsettings не должен решать за композитор"
        );
        assert!(p.steps.iter().any(|s| matches!(s, ApplyStep::WriteSetsOk { .. })), "{de}");
        let wm = p.steps.iter().find_map(|s| match s {
            ApplyStep::WmConfig(w) => Some(w.clone()),
            _ => None,
        });
        assert_eq!(wm.is_some(), TH::CursorWm::from_de(de).is_some(), "{de}");
    }
    // A name that must not be carried into someone else's config does not go there.
    let p = TH::apply_cursor_plan_sized("Bad\"Name", Some(24), "sway", HOME, None, true).unwrap();
    assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::WmConfig(_))));
}

/// river changes the pointer with its own command right away, Wayfire by its native key
/// `[input]` in its config (it rereads it itself); no config — we do not create one.
#[test]
fn river_and_wayfire_cursor_native_way() {
    let (calls, _) = run_plan(&sized("river"), false);
    assert!(calls.iter().any(|c| c.join(" ") == "riverctl xcursor-theme Bibata 32"), "{calls:?}");
    let plain = TH::apply_cursor_plan("Bibata", "river", HOME, true).unwrap();
    assert!(plain.steps.iter().any(|s| matches!(s, ApplyStep::Live(c) if c.argv == ["riverctl", "xcursor-theme", "Bibata"])));

    let wf = sized("wayfire");
    let guarded: Vec<(String, String, String, bool)> = wf
        .steps
        .iter()
        .filter_map(|s| match s {
            ApplyStep::IniSetGuarded { path, section, key, value, only_if_file, .. } if section == "[input]" => {
                Some((path.clone(), key.clone(), value.clone(), *only_if_file))
            }
            _ => None,
        })
        .collect();
    let want: Vec<(String, String, String, bool)> = ["/H/.config/wayfire/wayfire.ini", "/H/.config/wayfire.ini"]
        .iter()
        .flat_map(|p| {
            [
                (p.to_string(), "cursor_theme".to_string(), "Bibata".to_string(), true),
                (p.to_string(), "cursor_size".to_string(), "32".to_string(), true),
            ]
        })
        .collect();
    assert_eq!(guarded, want);
    // A key in wayfire.ini style (`key = value`) is edited in place, not
    // appended as a second one.
    let before = "[core]\nplugins = autostart\n\n[input]\ncursor_theme = default\nxkb_layout = us,ru\n";
    assert_eq!(
        TH::ini_guarded_text(Some(before), "[input]", "cursor_theme", "Bibata", true, false).as_deref(),
        Some("[core]\nplugins = autostart\n\n[input]\ncursor_theme=Bibata\nxkb_layout = us,ru\n")
    );
    assert_eq!(TH::ini_guarded_text(None, "[input]", "cursor_theme", "Bibata", true, false), None);
    // A similar key is not the same one.
    assert_eq!(
        crate_defaults::set_ini_value(Some("[input]\ncursor_theme_x = a\n"), "[input]", "cursor_theme", "B"),
        "[input]\ncursor_theme_x = a\ncursor_theme=B\n"
    );
    // Other desktops do not get these steps.
    for de in ["sway", "niri", "hyprland", "gnome", "kde"] {
        let p = sized(de);
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::Live(c) if c.argv[0] == "riverctl")), "{de}");
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::IniSetGuarded { section, .. } if section == "[input]")), "{de}");
    }
}

#[test]
fn wm_config_plans_per_compositor() {
    let sway = TH::wm_cursor_config_plan(TH::CursorWm::Sway, "Bibata Modern", Some(48), HOME, None);
    assert_eq!(sway.candidates[0].path, "/H/.sway/config");
    assert_eq!(sway.candidates[1].path, "/H/.config/sway/config");
    assert_eq!(sway.foreign, ["/H/.i3/config", "/H/.config/i3/config"]);
    assert_eq!(sway.seed.as_ref().unwrap().from, "/etc/sway/config");
    let f = &sway.candidates[1];
    assert!(f.own_content.ends_with("seat * xcursor_theme \"Bibata Modern\" 48\n"), "{}", f.own_content);
    assert_eq!(f.include_line, "include /H/.config/sway/rexon-cursor.conf");
    // sway strips ALL unescaped quotes from the `include` argument, and
    // then splits the path on spaces with wordexp: quotes reach wordexp
    // only escaped (`\'`), and the argument itself is in double quotes.
    let odd = TH::wm_cursor_config_plan(TH::CursorWm::Sway, "Bibata", None, "/home/a b'c$x", None);
    assert_eq!(
        odd.candidates[1].include_line,
        r#"include "\'/home/a b\'\\\'\'c$$x/.config/sway/rexon-cursor.conf\'""#
    );

    let niri = TH::wm_cursor_config_plan(TH::CursorWm::Niri, "Bibata", Some(48), HOME, None);
    assert_eq!(niri.candidates[0].path, "/H/.config/niri/config.kdl");
    assert!(niri.candidates[0].own_content.ends_with(
        "cursor {\n    xcursor-theme \"Bibata\"\n    xcursor-size 48\n}\n"
    ));
    assert_eq!(niri.candidates[0].include_line, "include \"rexon-cursor.kdl\"");
    assert_eq!(niri.min_version.as_ref().unwrap().min, (25, 11));

    let hypr = TH::wm_cursor_config_plan(TH::CursorWm::Hyprland, "Bibata", Some(48), HOME, Some("/X"));
    assert_eq!(hypr.candidates[0].path, "/X/hypr/hyprland.lua");
    assert_eq!(hypr.candidates[0].include_line, "pcall(require, \"rexon-cursor\")");
    assert!(hypr.candidates[0].own_content.contains("hl.env(\"XCURSOR_SIZE\", \"48\")"));
    assert!(hypr.candidates[0].own_content.contains("hl.env(\"HYPRCURSOR_THEME\", \"Bibata\")"));
    assert_eq!(hypr.candidates[1].path, "/X/hypr/hyprland.conf");
    assert_eq!(hypr.candidates[1].include_line, "source = /X/hypr/rexon-cursor.conf");
    assert!(hypr.candidates[1].own_content.contains("env = XCURSOR_THEME,Bibata\n"));

    assert_eq!(TH::parse_wm_version("niri 26.04 (8ed0da4)"), Some((26, 4)));
    assert_eq!(TH::parse_wm_version("niri 25.11"), Some((25, 11)));
    assert_eq!(TH::parse_wm_version("niri v0.1.10"), None);
    assert_eq!(TH::parse_wm_version("niri 0.1.10"), Some((0, 1)));
}

#[test]
fn managed_block_keeps_foreign_config() {
    let user = "set $mod Mod4\nbindsym $mod+Return exec foot\n";
    let once = TH::upsert_managed_block(user, "#", "cursor", "include /a");
    assert!(once.starts_with(user));
    assert!(once.ends_with(
        "\n# >>> Rexon: cursor (managed by Rexon, do not edit inside) >>>\ninclude /a\n# <<< Rexon: cursor <<<\n"
    ));
    // A repeat gives the same text; a new body takes the place of the old one.
    assert_eq!(TH::upsert_managed_block(&once, "#", "cursor", "include /a"), once);
    let twice = TH::upsert_managed_block(&once, "#", "cursor", "include /b");
    assert_eq!(twice.matches(">>> Rexon").count(), 1);
    assert!(twice.contains("include /b") && !twice.contains("include /a"));
    // The person's lines AFTER the block are not lost.
    let tail = format!("{once}bindsym $mod+q kill\n");
    let upd = TH::upsert_managed_block(&tail, "#", "cursor", "include /c");
    assert!(upd.ends_with("# <<< Rexon: cursor <<<\nbindsym $mod+q kill\n"));
    // Rollback restores the original byte-for-byte; without a trailing newline too.
    assert_eq!(TH::remove_managed_block(&once, "#", "cursor"), user);
    let bare = "a = 1";
    let b = TH::upsert_managed_block(bare, "//", "cursor", "include \"x\"");
    assert_eq!(TH::remove_managed_block(&b, "//", "cursor"), "a = 1\n");
    assert_eq!(TH::upsert_managed_block("", "--", "cursor", "x").lines().count(), 3);
}

#[test]
fn cursor_queries_and_values() {
    use TH::CursorQuery as Q;
    let kde = TH::cursor_queries("kde", HOME, None, true);
    assert_eq!(kde[0], Q::Ini { path: "/H/.config/kcminputrc".into(), section: "Mouse".into(), key: "cursorSize".into() });
    let sway = TH::cursor_queries("sway", HOME, None, false);
    assert_eq!(sway[0], Q::EnvFile { path: "/H/.config/environment.d/rexon-cursor.conf".into(), key: "XCURSOR_THEME".into() });
    assert!(matches!(&sway[1], Q::Cmd(c) if c.argv[2] == "org.gnome.desktop.interface"));
    let mate = TH::cursor_queries("mate", HOME, None, true);
    assert!(matches!(&mate[0], Q::Cmd(c) if c.argv[2..] == ["org.mate.peripherals-mouse", "cursor-size"]));
    let lxqt = TH::cursor_queries("lxqt", HOME, None, false);
    assert!(matches!(&lxqt[0], Q::Ini { key, .. } if key == "cursor_theme"));
    let xfce = TH::cursor_queries("xfce", HOME, None, true);
    assert!(matches!(&xfce[0], Q::Cmd(c) if c.argv.last().unwrap() == "/Gtk/CursorThemeSize"));

    assert_eq!(TH::clean_cursor_value("'Bibata'\n"), Some("Bibata".into()));
    assert_eq!(TH::parse_cursor_size("int32 48\n"), Some(48));
    assert_eq!(TH::parse_cursor_size("0"), None);
    assert_eq!(TH::ini_key_value("[Mouse]\ncursorSize=36\n", "Mouse", "cursorSize"), Some("36".into()));
    assert_eq!(TH::env_file_key_value("XCURSOR_THEME=A\nXCURSOR_SIZE=40\n", "XCURSOR_SIZE"), Some("40".into()));
}

/// The message tells the truth about where and when the cursor will change.
#[test]
fn cursor_messages_in_english() {
    use TH::{CursorReport as R, CursorWm as W, WmConfigOutcome as O};
    let _lang = i18n_pin::en();
    let wm = |w, o| R { ok: true, live: Some(true), wm: Some((w, o, "/H/.config/sway/config".into())), reason: None };
    assert_eq!(
        TH::cursor_apply_message("sway", &wm(W::Sway, O::Written)),
        "Cursor applied and saved in the sway config."
    );
    let not_live = R { live: Some(false), ..wm(W::Hyprland, O::Written) };
    assert_eq!(
        TH::cursor_apply_message("hyprland", &not_live),
        "Cursor saved in the Hyprland config. It shows up the next time you start Hyprland."
    );
    assert!(TH::cursor_apply_message("niri", &wm(W::Niri, O::Written)).contains("picks it up on its own"));
    assert!(TH::cursor_apply_message("sway", &wm(W::Sway, O::RolledBack)).contains("/H/.config/sway/config"));
    assert!(TH::cursor_apply_message("niri", &wm(W::Niri, O::TooOld)).contains("update niri"));
    let plain = R { ok: true, ..R::default() };
    assert_eq!(TH::cursor_apply_message("kde", &plain), "Cursor applied");
    assert!(TH::cursor_apply_message("gnome", &plain).contains("after you log in again"));
    assert!(TH::cursor_apply_message("lxqt", &plain).contains("LXQt"));
    // cosmic-comp takes XCURSOR_THEME at login, otherwise the `default` theme.
    assert!(TH::cursor_apply_message("cosmic", &plain).contains("compositor's own pointer"));
    let river_live = R { ok: true, live: Some(true), ..R::default() };
    assert_eq!(
        TH::cursor_apply_message("river", &river_live),
        "Cursor applied. Some Qt and Electron apps pick it up after you log in again."
    );
    assert!(TH::cursor_apply_message("river", &plain).contains("compositor's own pointer"));
    assert_eq!(
        TH::cursor_apply_message("gnome", &R::default()),
        "Cursor not applied (the theme may be unsafe)"
    );
}

// ── 15-bis. Executor: compositor configs on disk (private directory) ──
//
// Only the WmConfig/WriteSetsOk steps: gsettings, swaymsg and hyprctl from the plan are
// NOT run here — the check must not touch the live desktop.

fn tmp_home(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("rexon-cursor-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().into_owned()
}

fn run_wm_steps(theme: &str, de: &str, home: &str) -> TH::CursorReport {
    let plan = TH::apply_cursor_plan_sized(theme, Some(48), de, home, None, true).unwrap();
    let steps: Vec<ApplyStep> = plan
        .steps
        .into_iter()
        .filter(|s| matches!(s, ApplyStep::WmConfig(_) | ApplyStep::WriteSetsOk { .. }))
        .collect();
    rexon_core::appearance_installer::exec_apply_plan_report(&steps)
}

fn have_bin(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

#[test]
fn wm_config_executor_sway() {
    use TH::WmConfigOutcome as O;
    let home = tmp_home("sway");
    let cfg = format!("{home}/.config/sway/config");
    // No personal config, there is an i3 config — sway reads it; we do not touch it.
    std::fs::create_dir_all(format!("{home}/.config/i3")).unwrap();
    std::fs::write(format!("{home}/.config/i3/config"), "bindsym Mod4+q kill\n").unwrap();
    let r = run_wm_steps("Bibata", "sway", &home);
    assert!(r.ok, "environment.d записан — статус правдив");
    assert_eq!(r.wm.as_ref().unwrap().1, O::Foreign);
    assert_eq!(std::fs::read_to_string(format!("{home}/.config/i3/config")).unwrap(), "bindsym Mod4+q kill\n");
    assert!(!std::path::Path::new(&cfg).exists());
    std::fs::remove_dir_all(format!("{home}/.config/i3")).unwrap();

    // There is a config of its own: the block at the end, the rest in place, a copy before editing.
    std::fs::create_dir_all(format!("{home}/.config/sway")).unwrap();
    let user = "set $mod Mod4\nbindsym $mod+Return exec foot\nseat * xcursor_theme Adwaita 24\n";
    std::fs::write(&cfg, user).unwrap();
    let r = run_wm_steps("Bibata", "sway", &home);
    assert_eq!(r.wm.as_ref().unwrap().1, O::Written);
    let now = std::fs::read_to_string(&cfg).unwrap();
    assert!(now.starts_with(user));
    assert!(now.contains(&format!("include {home}/.config/sway/rexon-cursor.conf")));
    assert_eq!(std::fs::read_to_string(format!("{cfg}.rexon-bak")).unwrap(), user);
    assert!(std::fs::read_to_string(format!("{home}/.config/sway/rexon-cursor.conf"))
        .unwrap()
        .ends_with("seat * xcursor_theme Bibata 48\n"));
    assert_eq!(
        std::fs::read_to_string(format!("{home}/.config/environment.d/rexon-cursor.conf")).unwrap(),
        "XCURSOR_THEME=Bibata\nXCURSOR_SIZE=48\n"
    );
    // The second time the config is the same, one block.
    run_wm_steps("Bibata", "sway", &home);
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), now);
    // `sway --validate` still brings up the wlroots backend: without a session
    // (neither WAYLAND_DISPLAY nor DISPLAY — a build in a clean environment, CI) it
    // fails on grabbing the seat before even reading the config, and the check would go red
    // because of the machine, not the config.
    let has_display = ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    if have_bin("sway") && has_display {
        // The result is accepted by sway itself.
        let st = std::process::Command::new("sway").args(["--validate", "--config", &cfg]).output().unwrap();
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn wm_config_executor_niri_and_hyprland() {
    use TH::WmConfigOutcome as O;
    let home = tmp_home("niri");
    // No config — the compositor will create it itself; we do not create it (otherwise it
    // will not put its own one with the key bindings).
    let r = run_wm_steps("Bibata", "niri", &home);
    assert_eq!(r.wm.as_ref().unwrap().1, O::NoConfig);
    assert!(!std::path::Path::new(&format!("{home}/.config/niri/config.kdl")).exists());

    if have_bin("niri") && std::path::Path::new("/usr/share/doc/niri/default-config.kdl").is_file() {
        let cfg = format!("{home}/.config/niri/config.kdl");
        std::fs::create_dir_all(format!("{home}/.config/niri")).unwrap();
        let user = std::fs::read_to_string("/usr/share/doc/niri/default-config.kdl").unwrap();
        std::fs::write(&cfg, &user).unwrap();
        let r = run_wm_steps("Bibata", "niri", &home);
        assert_eq!(r.wm.as_ref().unwrap().1, O::Written);
        let now = std::fs::read_to_string(&cfg).unwrap();
        assert!(now.starts_with(&user) && now.contains("include \"rexon-cursor.kdl\""));
        let st = std::process::Command::new("niri").args(["validate", "--config", &cfg]).output().unwrap();
        assert!(st.status.success());

        // The niri check rejected the result (our file broken) — the person's config
        // is restored as it was.
        std::fs::write(&cfg, &user).unwrap();
        std::fs::write(format!("{cfg}.rexon-bak"), &user).unwrap();
        let plan = TH::wm_cursor_config_plan(TH::CursorWm::Niri, "Bibata", Some(48), &home, None);
        let mut broken = plan.clone();
        broken.candidates[0].own_content = "cursor { xcursor-size \"x\" }\n".into();
        let r = rexon_core::appearance_installer::exec_apply_plan_report(&[ApplyStep::WmConfig(broken)]);
        assert_eq!(r.wm.as_ref().unwrap().1, O::RolledBack);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), user);
    }

    if have_bin("Hyprland") && std::path::Path::new("/usr/share/hypr/hyprland.lua").is_file() {
        let cfg = format!("{home}/.config/hypr/hyprland.lua");
        std::fs::create_dir_all(format!("{home}/.config/hypr")).unwrap();
        let user = std::fs::read_to_string("/usr/share/hypr/hyprland.lua").unwrap();
        std::fs::write(&cfg, &user).unwrap();
        let r = run_wm_steps("Bibata", "hyprland", &home);
        assert_eq!(r.wm.as_ref().unwrap().1, O::Written);
        assert!(std::fs::read_to_string(&cfg).unwrap().ends_with(
            "pcall(require, \"rexon-cursor\")\n-- <<< Rexon: cursor <<<\n"
        ));
        assert!(std::path::Path::new(&format!("{home}/.config/hypr/rexon-cursor.lua")).is_file());
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// The size chips in «Appearance» are the same numbers the cursor core knows.
#[test]
fn cursor_size_chips_match_core() {
    let keys: Vec<u32> = rexon_core::appearance_page::CURSOR_SIZE_KEYS
        .iter()
        .map(|k| k.parse().unwrap())
        .collect();
    assert_eq!(keys, TH::CURSOR_SIZES);
    assert!(TH::CURSOR_SIZES.contains(&TH::CURSOR_DEFAULT_SIZE));
}

/// In Russian — from the real `po/ru.po`.
#[test]
fn cursor_messages_in_russian() {
    use TH::{CursorReport as R, CursorWm as W, WmConfigOutcome as O};
    let _lang = i18n_pin::ru();
    let r = R { ok: true, live: Some(true), wm: Some((W::Sway, O::Written, String::new())), reason: None };
    assert_eq!(TH::cursor_apply_message("sway", &r), "Курсор применён и сохранён в конфиге sway.");
    let plain = R { ok: true, ..R::default() };
    assert_eq!(
        TH::cursor_apply_message("gnome", &plain),
        "Курсор применён. Часть программ на Qt и Electron подхватит его после повторного входа."
    );
    assert_eq!(rexon_core::appearance_page::cursor_size_label(), "Размер:");
}

/// The whole stand: `LiveInstaller::apply_cursor_size` in a private HOME.
/// Run only by hand and only in a separate environment (private HOME,
/// XDG_CONFIG_HOME and session bus — `dbus-run-session`), otherwise gsettings
/// and the configs would reach the live desktop:
/// `REXON_CURSOR_STAND=<de> HOME=… XDG_CONFIG_HOME=… dbus-run-session cargo test …
///  --test themes_parity cursor_stand -- --ignored --nocapture`.
#[test]
#[ignore]
fn cursor_stand() {
    let Ok(de) = std::env::var("REXON_CURSOR_STAND") else {
        return;
    };
    let home = std::env::var("HOME").unwrap();
    assert!(home.contains("rexon-stand"), "стенд только в приватном HOME, не в {home}");
    let mut inst = rexon_core::appearance_installer::LiveInstaller::new(&de, Box::new(|t: &str| eprintln!("лог: {t}")));
    let (ok, msg) = inst.apply_cursor_size(48);
    eprintln!("{de}: ok={ok} msg={msg}");
    eprintln!("{de}: размер теперь {:?}", inst.cursor_size());
}

/// D3 (desktop check 02.10.2026): on KDE a change of style and Kvantum
/// is announced to running programs with the StyleChanged signal (2), not
/// PaletteChanged (0) — with 0 the program stayed on the old style.
#[test]
fn kde_style_change_sends_style_changed() {
    let notify = |p: &Plan| -> Vec<String> {
        p.steps
            .iter()
            .find_map(|s| match s {
                ApplyStep::IfOk(c) if c.argv[0] == "dbus-send" => Some(c.argv.clone()),
                _ => None,
            })
            .expect("уведомление Plasma")
    };
    for plan in [ws_plan("kde", ""), kv_plan("kde", "")] {
        assert_eq!(
            notify(&plan),
            [
                "dbus-send", "--type=signal", "/KGlobalSettings",
                "org.kde.KGlobalSettings.notifyChange", "int32:2", "int32:0",
            ]
        );
    }
}

fn svec(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// D4 (desktop check 02.10.2026): under Plasma icons and the GTK theme reach
/// GTK programs right away — icons are written with `kwriteconfig6 --notify` (it is
/// listened to by the `gtkconfig` module), the GTK theme is handed to `gtkconfig` itself.
#[test]
fn kde_icons_and_gtk_reach_gtkconfig() {
    let argvs = |p: &Plan| -> Vec<Vec<String>> {
        p.steps
            .iter()
            .filter_map(|s| match s {
                ApplyStep::Primary(c) | ApplyStep::BestEffort(c) | ApplyStep::IfOk(c) => Some(c.argv.clone()),
                _ => None,
            })
            .collect()
    };
    let icons = TH::apply_icon_plan("Papirus", "kde", HOME, true).unwrap();
    assert!(argvs(&icons).contains(&svec(&[
        "kwriteconfig6", "--file", "kdeglobals", "--group", "Icons", "--key", "Theme", "--notify", "Papirus",
    ])));
    let gtk = TH::apply_gtk_plan("Materia-dark", "kde", HOME);
    assert!(argvs(&gtk).contains(&svec(&[
        "dbus-send", "--session", "--print-reply", "--dest=org.kde.GtkConfig", "/GtkConfig",
        "org.kde.GtkConfig.setGtkTheme", "string:Materia-dark",
    ])));
    // Outside KDE there is no Plasma module — and no call.
    for de in ["gnome", "xfce", "lxqt", "sway"] {
        assert!(!argvs(&TH::apply_gtk_plan("Materia-dark", de, HOME))
            .iter()
            .any(|a| a.iter().any(|x| x.contains("GtkConfig"))), "{de}");
    }
}

/// D2 (desktop check 02.10.2026): under qt6ct Qt programs take icons
/// from its config. The icon choice is written there if the config exists; a config
/// created by Rexon gets the desktop's current icons, not emptiness (live it was
/// Papirus → hicolor).
#[test]
fn qt6ct_keeps_icon_theme() {
    // The icons plan: into qt6ct.conf and qt5ct.conf only if the file already exists;
    // kdeglobals (KDE programs outside Plasma) always.
    let icons = TH::apply_icon_plan("Papirus", "gnome", HOME, true).unwrap();
    let guarded: Vec<_> = icons
        .steps
        .iter()
        .filter_map(|s| match s {
            ApplyStep::IniSetGuarded { path, key, value, only_if_file, keep_existing, .. } => {
                Some((path.clone(), key.clone(), value.clone(), *only_if_file, *keep_existing))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        guarded,
        [
            ("/H/.config/kdeglobals".to_string(), "Theme".to_string(), "Papirus".to_string(), false, false),
            ("/H/.config/qt6ct/qt6ct.conf".to_string(), "icon_theme".to_string(), "Papirus".to_string(), true, false),
            ("/H/.config/qt5ct/qt5ct.conf".to_string(), "icon_theme".to_string(), "Papirus".to_string(), true, false),
        ]
    );
    // No file — we do not create one; there is one — we edit it, the rest is untouched.
    assert_eq!(TH::ini_guarded_text(None, "[Appearance]", "icon_theme", "Papirus", true, false), None);
    assert_eq!(
        TH::ini_guarded_text(Some("[Appearance]\nstyle=Fusion\nicon_theme=breeze\n"), "[Appearance]", "icon_theme", "Papirus", true, false)
            .as_deref(),
        Some("[Appearance]\nstyle=Fusion\nicon_theme=Papirus\n")
    );

    // Style via qt6ct: the desktop's current icons land if there are none of its own.
    let env = TH::KvantumEnv {
        de_id: "gnome",
        home: "/home/t",
        xdg_config_home: "",
        platform_theme: "qt6ct",
        icon_theme: "Papirus".into(),
        qt_desktop_installed: false,
    };
    let plan = TH::apply_qt_style_plan("Fusion", &env);
    assert!(plan.steps.contains(&ApplyStep::IniSetGuarded {
        path: "/home/t/.config/qt6ct/qt6ct.conf".into(),
        section: "[Appearance]".into(),
        key: "icon_theme".into(),
        value: "Papirus".into(),
        only_if_file: false,
        keep_existing: true,
    }));
    assert_eq!(
        TH::ini_guarded_text(None, "[Appearance]", "icon_theme", "Papirus", false, true).as_deref(),
        Some("\n[Appearance]\nicon_theme=Papirus\n")
    );
    // The person has already made their own choice in qt6ct — we do not override it.
    assert_eq!(
        TH::ini_guarded_text(Some("[Appearance]\nicon_theme=breeze\n"), "[Appearance]", "icon_theme", "Papirus", false, true),
        None
    );
    // On KDE and LXQt qt6ct is irrelevant — no step.
    for de in ["kde", "lxqt"] {
        let env = TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme: "qt6ct", icon_theme: "Papirus".into(), qt_desktop_installed: false };
        assert!(!TH::apply_qt_style_plan("Fusion", &env).steps.iter().any(|s| matches!(s, ApplyStep::IniSetGuarded { .. })), "{de}");
    }
    // The source of the current icons is the same keys the icons plan writes.
    assert_eq!(TH::parse_icon_theme_output("'Papirus'\n"), "Papirus");
    match TH::current_icon_theme_source("xfce", "/h", None) {
        TH::IconThemeSource::Cmd(c) => assert_eq!(c.argv[..3], ["xfconf-query", "-c", "xsettings"]),
        other => panic!("{other:?}"),
    }
}

/// «Kvantum in GNOME» must not reach Plasma: while a desktop with its own Qt
/// integration is installed, the variables do not go into the shared
/// `environment.d` (Plasma imports it), but into Rexon's desktop-bound
/// directory, and the shared file is removed. Without such a desktop — as before.
#[test]
fn qt_style_variables_do_not_leak_into_plasma() {
    for (tool, var) in [("qt6ct", "QT_QPA_PLATFORMTHEME=qt6ct\n"), ("", "QT_STYLE_OVERRIDE=kvantum\n")] {
        let env = TH::KvantumEnv {
            de_id: "gnome",
            home: "/home/t",
            xdg_config_home: "",
            platform_theme: tool,
            icon_theme: String::new(),
            qt_desktop_installed: true,
        };
        let plan = TH::apply_kvantum_plan("KvArc", &env);
        let writes: Vec<(&str, &str)> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                ApplyStep::WriteIfOk { path, content, .. } => Some((path.as_str(), content.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(writes, [("/home/t/.config/rexon/session-env/rexon-qt-style.conf", var)], "{tool}");
        assert!(plan.steps.contains(&ApplyStep::RemoveIfOk {
            path: "/home/t/.config/environment.d/rexon-qt-style.conf".into()
        }));
        // Without such a desktop the shared file is right: GNOME and COSMIC read only it.
        let env = TH::KvantumEnv { qt_desktop_installed: false, ..env };
        let plan = TH::apply_kvantum_plan("KvArc", &env);
        assert!(plan.steps.iter().any(|s| matches!(s,
            ApplyStep::WriteIfOk { path, .. } if path == "/home/t/.config/environment.d/rexon-qt-style.conf")));
    }
    // Which desktops count: Plasma, LXQt, Deepin — by their session programs.
    assert!(TH::own_qt_desktop_installed(&|b| b == "plasmashell"));
    assert!(TH::own_qt_desktop_installed(&|b| b == "lxqt-session"));
    assert!(TH::own_qt_desktop_installed(&|b| b == "dde-session"));
    assert!(!TH::own_qt_desktop_installed(&|b| b == "gnome-shell" || b == "cosmic-session"));
}

/// Icons in all desktops: without dconf (`gsettings` fails) tiling desktops and LXQt
/// still hand the set to Qt programs — qt6ct and kdeglobals; KDE programs
/// get it everywhere except Plasma itself (its own step writes there) and Deepin.
#[test]
fn icons_reach_qt_apps_without_dconf() {
    let qt6ct = "/H/.config/qt6ct/qt6ct.conf";
    for de in ["sway", "hyprland", "niri", "river", "wayfire", "lxqt"] {
        let plan = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        let force = plan.steps.iter().position(|s| matches!(s, ApplyStep::ForceOk)).expect(de);
        let guarded = plan.steps.iter().position(|s| matches!(s, ApplyStep::IniSetGuarded { .. })).expect(de);
        assert!(force < guarded, "{de}: ForceOk после записей «если ok»");
        // Execution with a failed gsettings and an already configured qt6ct.
        let mut ok = false;
        let mut files: BTreeMap<String, String> = BTreeMap::from([(qt6ct.to_string(), "[Appearance]\nstyle=Fusion\n".to_string())]);
        for step in &plan.steps {
            match step {
                ApplyStep::Primary(c) => ok = c.argv[0] != "gsettings",
                ApplyStep::ForceOk => ok = true,
                ApplyStep::IniSetGuarded { path, section, key, value, only_if_file, keep_existing } if ok => {
                    if let Some(t) = TH::ini_guarded_text(files.get(path).map(String::as_str), section, key, value, *only_if_file, *keep_existing) {
                        files.insert(path.clone(), t);
                    }
                }
                _ => {}
            }
        }
        assert_eq!(files[qt6ct], "[Appearance]\nstyle=Fusion\nicon_theme=Papirus\n", "{de}");
        assert_eq!(files["/H/.config/kdeglobals"], "\n[Icons]\nTheme=Papirus\n", "{de}");
    }
    for de in ["gnome", "cinnamon-mint", "mate", "xfce", "budgie", "pantheon", "cosmic"] {
        let plan = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        assert!(plan.steps.iter().any(|s| matches!(s, ApplyStep::IniSetGuarded { path, key, .. }
            if path == "/H/.config/kdeglobals" && key == "Theme")), "{de}: нет kdeglobals");
    }
    for de in ["kde", "deepin"] {
        let plan = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        assert!(!plan.steps.iter().any(|s| matches!(s, ApplyStep::IniSetGuarded { path, .. } if path.ends_with("kdeglobals"))), "{de}");
    }
    // The rest of kdeglobals is kept, only our own key is edited.
    assert_eq!(
        TH::ini_guarded_text(Some("[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=breeze\n"), "[Icons]", "Theme", "Papirus", false, false).as_deref(),
        Some("[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=Papirus\n")
    );
}

// ── The desktop's standard cursor (Adwaita in GNOME and Cinnamon) ──────────────

/// The role's standard theme is learned from the schema itself (default + distribution
/// overrides), not from a table; where there is no schema there is nothing to ask.
#[test]
fn standard_theme_read_cmd_asks_schema_default() {
    let argv = |de: &str, role: &str| TH::standard_theme_read_cmd(de, role).map(|c| c.argv);
    let mem = |schema: &str, key: &str| {
        Some(
            ["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", schema, key]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(argv("gnome", "cursor"), mem("org.gnome.desktop.interface", "cursor-theme"));
    assert_eq!(argv("gnome", "icons"), mem("org.gnome.desktop.interface", "icon-theme"));
    assert_eq!(argv("cinnamon-mint", "cursor"), mem("org.cinnamon.desktop.interface", "cursor-theme"));
    assert_eq!(argv("mate", "cursor"), mem("org.mate.peripherals-mouse", "cursor-theme"));
    for de in ["kde", "xfce", "deepin", "lxqt", "sway"] {
        assert_eq!(argv(de, "cursor"), None, "{de}");
    }
}

/// Returning to the standard cursor: the native key is reset rather than written
/// with a theme name; our explicit settings (GTK settings.ini, environment.d)
/// are removed; the name of an unsafe theme is in no record. Cinnamon —
/// with its own schema.
#[test]
fn standard_cursor_plan_resets_instead_of_writing_unsafe_name() {
    for (de, schema) in [("gnome", "org.gnome.desktop.interface"), ("cinnamon-mint", "org.cinnamon.desktop.interface")] {
        let plan = TH::standard_cursor_plan(de, Some(32), HOME, None).expect(de);
        let ApplyStep::Primary(c) = &plan.steps[0] else { panic!("{:?}", plan.steps) };
        assert_eq!(c.argv, ["gsettings", "reset", schema, "cursor-theme"]);
        assert!(plan.steps.iter().any(|s| matches!(s, ApplyStep::BestEffort(c) if c.argv == ["gsettings", "set", schema, "cursor-size", "32"])));
        for v in ["gtk-3.0", "gtk-4.0"] {
            assert!(plan.steps.iter().any(|s| matches!(s,
                ApplyStep::GtkIniUnsetIfOk { path, key } if path == &format!("{HOME}/.config/{v}/settings.ini") && key == "gtk-cursor-theme-name")));
        }
        assert!(plan.steps.iter().any(|s| matches!(s,
            ApplyStep::RemoveIfOk { path } if path == &format!("{HOME}/.config/environment.d/{}", TH::CURSOR_ENV_FILE))));
        assert!(!format!("{:?}", plan.steps).contains("Adwaita"), "{de}");
    }
    for de in ["kde", "xfce", "sway", "hyprland-unknown"] {
        assert!(TH::standard_cursor_plan(de, None, HOME, None).is_none(), "{de}");
    }
    // The protection is in place: a normal plan for an unsafe theme is still not built.
    assert!(TH::apply_cursor_plan_sized("Adwaita", None, "gnome", HOME, None, false).is_none());
}

#[test]
fn remove_gtk_settings_key_keeps_the_rest() {
    let ini = "[Settings]\ngtk-theme-name=Dracula\ngtk-cursor-theme-name=Bibata\ngtk-cursor-theme-size=32\n";
    assert_eq!(
        TH::remove_gtk_settings_key(ini, "gtk-cursor-theme-name").as_deref(),
        Some("[Settings]\ngtk-theme-name=Dracula\ngtk-cursor-theme-size=32\n")
    );
    assert_eq!(TH::remove_gtk_settings_key("[Settings]\n", "gtk-cursor-theme-name"), None);
}

// ── Program mode (color-scheme) following the GTK theme ───────────────────────

/// Whether the theme is dark: the window background color from gtk.css (following `@name` references), without it —
/// "dark" in the name; the color matters more than the name.
/// MATE themes keep their colors in a file `gtk.css` imports (BlackMATE: only
/// `@import url("gtk-main.css");`); without following it the dark BlackMATE
/// counted as light by its name. Resources and paths outside the theme are left out.
#[test]
fn gtk_css_local_imports_names_the_theme_files() {
    let css = "@import url(\"gtk-main.css\");\n@import 'gtk-widgets.css';\n\
               @import url(\"resource:///com/ubuntu/themes/Yaru/3.0/gtk.css\");\n\
               @import url(\"/usr/share/x.css\");\n@import url(\"../up.css\");\n";
    assert_eq!(TH::gtk_css_local_imports(css), vec!["gtk-main.css".to_string(), "gtk-widgets.css".to_string()]);
    let main = "@define-color bg_color #3b3b3e;\n@define-color theme_bg_color @bg_color;\n";
    assert!(TH::gtk_theme_is_dark("BlackMATE", Some(&format!("@import url(\"gtk-main.css\");\n{main}"))));
}

#[test]
fn gtk_theme_is_dark_by_css_then_name() {
    let dracula = "@define-color bg_color #1e1f29;\n@define-color theme_bg_color @bg_color;\n";
    assert!(TH::gtk_theme_is_dark("Dracula", Some(dracula)));
    let adw = "@define-color window_bg_color #222226;\n@define-color theme_bg_color @window_bg_color;\n";
    assert!(TH::gtk_theme_is_dark("adw-gtk3-dark", Some(adw)));
    assert!(!TH::gtk_theme_is_dark("Materia", Some("@define-color theme_bg_color #f9f9f9;\n")));
    // The name says "Dark" but the window background is light — the color decides.
    let royale = "@define-color bg_color #ebe9ed;\n@define-color theme_bg_color @bg_color;\n";
    assert!(!TH::gtk_theme_is_dark("Windows XP Royale Dark", Some(royale)));
    // Styles in a GTK resource — by name.
    let import = "@import url(\"resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained-dark.css\");\n";
    assert!(TH::gtk_theme_is_dark("Adwaita-dark", Some(import)));
    assert!(!TH::gtk_theme_is_dark("Adwaita", None));
    // A reference loop does not hang the parsing — the name decides.
    assert!(!TH::gtk_theme_is_dark("Loop", Some("@define-color theme_bg_color @a;\n@define-color a @theme_bg_color;\n")));
}

/// A dark GTK theme sets `prefer-dark` for programs, a light one `default`;
/// only where the GTK theme is written into the GNOME schema, and only after the theme
/// is written. Cinnamon, MATE, XFCE, KDE manage the mode themselves.
#[test]
fn gtk_plan_sets_color_scheme_where_gnome_schema() {
    let scheme = |de: &str, dark: Option<bool>| -> Vec<Vec<String>> {
        TH::apply_gtk_plan_scheme("T", de, HOME, None, dark)
            .steps
            .iter()
            .filter_map(|s| match s {
                ApplyStep::IfOk(c) if c.argv.iter().any(|a| a == "color-scheme") => Some(c.argv.clone()),
                ApplyStep::Primary(c) | ApplyStep::BestEffort(c) if c.argv.iter().any(|a| a == "color-scheme") => {
                    panic!("{de}: режим не должен решать ok: {c:?}")
                }
                _ => None,
            })
            .collect()
    };
    let set = |v: &str| vec![vec!["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", v]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()];
    // Pantheon also writes its service's key —
    // `pantheon_dark_mode_goes_to_elementary_settings_daemon`.
    for de in ["gnome", "budgie", "cosmic", "sway"] {
        assert_eq!(scheme(de, Some(true)), set("prefer-dark"), "{de}");
        assert_eq!(scheme(de, Some(false)), set("default"), "{de}");
        assert!(scheme(de, None).is_empty(), "{de}");
    }
    for de in ["cinnamon-mint", "mate", "xfce", "kde", "deepin"] {
        assert!(scheme(de, Some(true)).is_empty(), "{de}");
    }
    // The old plan without a mode did not change.
    assert_eq!(TH::apply_gtk_plan_xdg("T", "gnome", HOME, None), TH::apply_gtk_plan_scheme("T", "gnome", HOME, None, None));
}

/// Pantheon: the light/dark mode is managed by the io.elementary.settings-daemon service
/// through ITS OWN key `io.elementary.settings-daemon.prefers-color-scheme
/// color-scheme` (written by «System Settings → Desktop → Appearance»); from it
/// the service hands the mode out to AccountsService — Gala, the panel and
/// elementary programs take it from there — and into the GNOME key. The GNOME key alone left the shell
/// light, and on the dark mode schedule the service rewrote it back.
#[test]
fn pantheon_dark_mode_goes_to_elementary_settings_daemon() {
    let ifok = |dark: bool| -> Vec<Vec<String>> {
        TH::apply_gtk_plan_scheme("T", "pantheon", HOME, None, Some(dark))
            .steps
            .iter()
            .filter_map(|s| match s {
                ApplyStep::IfOk(c) => Some(c.argv.clone()),
                _ => None,
            })
            .collect()
    };
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let elementary = |val: &str| {
        v(&["gsettings", "set", "io.elementary.settings-daemon.prefers-color-scheme", "color-scheme", val])
    };
    let dark = ifok(true);
    assert!(dark.contains(&elementary("prefer-dark")), "{dark:?}");
    assert!(dark.contains(&v(&["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", "prefer-dark"])));
    // "Light" for elementary is `no-preference` (the «Default» button).
    assert!(ifok(false).contains(&elementary("no-preference")));
    // Other desktops do not get the elementary key.
    for de in ["gnome", "budgie", "cosmic"] {
        let plan = TH::apply_gtk_plan_scheme("T", de, HOME, None, Some(true));
        assert!(!format!("{:?}", plan.steps).contains("io.elementary"), "{de}");
    }
}
