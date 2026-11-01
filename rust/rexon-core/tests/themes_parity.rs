//! Golden-сверка порта `themes.rs` с оригиналом `rexon/themes.py`
//! (класс `ThemeManager`).
//!
//! Эталоны сняты с ЖИВОГО Python read-only (одноразовым генератором, в дерево не входит):
//! `safe_run`/`safe_check_command` замоканы (никаких реальных subprocess/
//! привилегированных действий), HOME — временный каталог, встроенный каталог
//! тем — реальный `rexon/themes.json`. Покрыто: слияние каталога, разрешение
//! gsettings-схем + generic-wm, план apply GTK/иконок/курсоров для разных DE
//! (+ kde-cursor fallback), запись `settings.ini`, разбор `index.theme` /
//! `kcminputrc`, шрифтовые хелперы, разрешение пакета установки, «текущие
//! темы», KDE-планы apply, история.


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

// ── 1. Слияние каталога (байт-в-байт) ─────────────────────────────────────

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
                // Порядок ролей должен совпадать (Python dict order).
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

// ── 4. apply-планы: интерпретируем план → (calls, files), сверяем с Python ─

fn cmd_success(argv: &[String], fail_mode: bool) -> bool {
    if fail_mode && argv.first().map(String::as_str) == Some("plasma-apply-cursortheme") {
        return false;
    }
    true
}

/// Прогнать план как исполнитель: собрать список argv-вызовов и итоговые файлы
/// (относительно HOME). `fail_mode` — режим падения (kde-cursor).
fn run_plan(plan: &Plan, fail_mode: bool) -> (Vec<Vec<String>>, BTreeMap<String, String>) {
    let mut calls: Vec<Vec<String>> = Vec::new();
    let mut vfs: BTreeMap<String, String> = BTreeMap::new();
    let mut ok = false;
    // Для группы IfNotOk: решение «выполнять» фиксируется на входе в группу.
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
            // Конфиг композитора исполнитель трогает только на диске —
            // отдельная сверка `wm_config_executor_*`.
            ApplyStep::WmConfig(_) => fallback_run = None,
            ApplyStep::ForceOk => {
                fallback_run = None;
                ok = true;
            }
            // Стоп Deepin — отдельные проверки (`dde.rs`, `appearance_installer.rs`).
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
            // Планы тем этих шагов не дают: они у «Применить» звука, обоев
            // блокировки и разделов окружения (`appearance_apply_plans_parity`).
            ApplyStep::Guard { .. } | ApplyStep::ReadBackSetsOk { .. } => fallback_run = None,
        }
    }

    // Пути → относительные HOME (как эталон Python).
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
            // valid/safe = true (в генераторе pre-flight замокан на допуск).
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
        // Намеренное расхождение с Python: на LXQt тема GTK теперь пишется и
        // в settings.ini (как у lxqt-config-appearance) — без xsettingsd GTK
        // читает именно файл, а gsettings оставался без читателя.
        // Намеренное расхождение с Python (30.09): курсор вне KDE получают и
        // программы, читающие XCURSOR_* из окружения (Qt без платформенной
        // темы, Electron, XWayland) — файл environment.d.
        if kind == "cursor" && de != "kde" {
            want_files.insert(
                ".config/environment.d/rexon-cursor.conf".to_string(),
                format!("XCURSOR_THEME={theme}\n"),
            );
        }
        // Намеренное расхождение с Python (24.10): значки вне KDE получают и
        // программы KDE (KIconThemes читает `kdeglobals [Icons] Theme`).
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

    // Отказ pre-flight: невалидная icon-тема / unsafe cursor → плана нет.
    assert!(TH::apply_icon_plan("Cursorish", "gnome", HOME, false).is_none());
    assert!(TH::apply_cursor_plan("Adwaita", "gnome", HOME, false).is_none());
}

// ── 5. is_valid_icon_theme (разбор index.theme) ───────────────────────────

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
    // пустое имя
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

// ── 9. KDE-планы apply + get_current_kde_cmds ─────────────────────────────

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

    // kvantum на KDE: set → widget_style → notify (эталон снят с KDE-пути,
    // он и остался нетронутым; вне KDE второй шаг другой — см. модульные
    // тесты kvantum_* в themes.rs).
    let kv = TH::apply_kvantum_plan(
        "KvArc",
        &TH::KvantumEnv {
            de_id: "kde",
            home: "/home/t",
            xdg_config_home: "",
            platform_theme: "",
            icon_theme: String::new(),
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

// ── 9-bis. Сканеры установленных тем (снимок ФС) ──────────────────────────

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

    // kvantum: dir_files — только файлы внутри каталога темы.
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

// ── 10. история (детерминированная логика) ────────────────────────────────

#[test]
fn history_parity() {
    let f = fx();
    let ts = f["history_ts"].as_str().unwrap();
    let e = TH::history_entry("gtk", "Dracula", "kde", ts);
    assert_eq!(
        e,
        json!({"kind": "gtk", "theme": "Dracula", "de": "kde", "ts": ts})
    );

    // push: вставка в начало.
    let h0: Vec<Value> = Vec::new();
    let h1 = TH::push_history(&h0, e.clone());
    assert_eq!(h1, vec![e.clone()]);
    let e2 = TH::history_entry("icons", "Papirus", "kde", ts);
    let h2 = TH::push_history(&h1, e2.clone());
    assert_eq!(h2, vec![e2.clone(), e.clone()]);

    // truncate до 50.
    let mut big: Vec<Value> = (0..60).map(|i| json!({"n": i})).collect();
    let pushed = TH::push_history(&big, e.clone());
    assert_eq!(pushed.len(), 50);
    assert_eq!(pushed[0], e);
    assert_eq!(pushed[1], json!({"n": 0}));

    // get_recent.
    big.truncate(5);
    assert_eq!(TH::get_recent(&big, 3).len(), 3);
    assert_eq!(TH::get_recent(&big, 100).len(), 5);

    // serialize — валидный JSON-массив (ensure_ascii=False, indent=2).
    let s = TH::serialize_history(&h2);
    assert!(s.starts_with("[\n"));
    assert!(s.contains("\"theme\": \"Papirus\""));
}

// ══════════════════════════════════════════════════════════════════════════
//  Вторая волна порта: архив темы, тексты каталога, чтение kdeglobals
// ══════════════════════════════════════════════════════════════════════════
//
// Эталон `pure_v2` снят с живого Python на данных ЭТОЙ машины: настоящий
// `~/.config/kdeglobals` (значения — через `configparser(strict=False)`, как в
// фолбэке `_kde_get`) и настоящие имена каталогов тем из `~/.themes` и
// `/usr/share/themes` (с пробелами и кириллицей — проверяем сортировку).

fn strs2(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn texts_match_python() {
    // Эталон снят с русского интерфейса — сверка на русском каталоге.
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

// ── 14. Kvantum вне KDE: тема ставится везде, стиль Qt — своим путём ───────
//
// Эталона в Python у этих веток нет: до сих пор второй шаг умели делать
// только для KDE, а в остальных окружениях раздел просто прятали. Проверяем
// не байтовый паритет, а обещания: где какой механизм и что человек без
// результата не остаётся.

fn kv_plan(de: &str, platform_theme: &str) -> Plan {
    TH::apply_kvantum_plan(
        "KvArc",
        &TH::KvantumEnv {
            de_id: de,
            home: "/home/t",
            xdg_config_home: "",
            platform_theme,
            icon_theme: String::new(),
        },
    )
}

/// Первый шаг один и тот же везде: сам выбор темы Kvantum.
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

/// KDE и LXQt правят СВОИ настройки; переменной окружения там быть не должно
/// — она перебила бы штатный выбор стиля.
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

/// Полный путь: qt6ct есть — стиль пишется в его конфиг, а переменная
/// назначает его платформенной темой (тогда будут и шрифты, и значки).
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
    // QT_STYLE_OVERRIDE тут НЕ пишем: он перебил бы выбор стиля в самом qt6ct.
    assert!(!env.1.contains("QT_STYLE_OVERRIDE"));
}

/// Запасной путь полноправен: qt6ct нет (отказались, отменили пароль, пакета
/// нет в репозиториях) — тема всё равно применяется.
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
        // Чужие конфиги без qt6ct не трогаем вовсе.
        assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::IniSetIfOk { .. })));
    }
}

/// `$XDG_CONFIG_HOME` уважается — иначе файл ляжет мимо сеанса.
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
        },
    );
    assert!(p.steps.iter().any(|s| matches!(
        s, ApplyStep::WriteIfOk { path, .. }
        if path == "/home/t/cfg/environment.d/rexon-qt-style.conf"
    )));
}

/// Имя пакета берётся по семейству дистрибутива, а не «везде qt6ct», и
/// берётся из ОБЩЕЙ таблицы пакетов, а не из своей второй копии.
#[test]
fn qt_style_helper_package_is_per_family() {
    let pkg = |f: &str| TH::qt_style_helper_pkg(f);
    for family in ["arch", "debian", "fedora", "suse", "alpine", "void", "solus", "alt"] {
        assert_eq!(pkg(family).as_deref(), Some("qt6ct"), "{family}");
    }
    // Gentoo хранит имена с категорией.
    assert_eq!(pkg("gentoo").as_deref(), Some("gui-apps/qt6ct"));
    // Ставить нечем — доустановки не будет, сработает запасной путь.
    assert_eq!(pkg("nix"), None);
    assert_eq!(pkg("clear"), None);
    // Незнакомое семейство — пробуем самое ходовое имя: промах безвреден,
    // тема применится запасным путём.
    assert_eq!(pkg("какое-то-новое").as_deref(), Some("qt6ct"));
    // Та же таблица, что у остальных инструментов.
    assert_eq!(
        rexon_core::utils::tool_package_name(TH::QT_STYLE_TOOL, "gentoo"),
        "gui-apps/qt6ct"
    );
}

// ── 14-bis. «Стиль приложений» — та же развилка, что у Kvantum ─────────────
//
// Таблица «окружение → что пишется»: KDE — kdeglobals + уведомление, LXQt —
// lxqt.conf [Qt] style, остальные — qt6ct.conf + environment.d (или
// QT_STYLE_OVERRIDE без qt6ct). Раньше раздел писал kdeglobals везде.

fn ws_plan(de: &str, platform_theme: &str) -> Plan {
    TH::apply_qt_style_plan(
        "Fusion",
        &TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme, icon_theme: String::new() },
    )
}

/// Сводная таблица: какой файл и какой ключ получает стиль в каждом окружении.
#[test]
fn widget_style_table_by_desktop() {
    // (окружение, qt6ct, ожидаемые записи «путь|секция|ключ=значение» или argv)
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
            ],
            "{de} с qt6ct"
        );
        assert_eq!(
            describe(&ws_plan(de, "")),
            [
                "forceok",
                "write:/home/t/.config/environment.d/rexon-qt-style.conf|QT_STYLE_OVERRIDE=Fusion",
            ],
            "{de} без qt6ct"
        );
        // kdeglobals вне KDE больше не пишется.
        assert!(!describe(&ws_plan(de, "")).iter().any(|s| s.contains("kdeglobals")), "{de}");
    }
}

/// Kvantum и «Стиль приложений» — одна развилка: хвост Kvantum-плана совпадает
/// с планом стиля «kvantum», отличаясь лишь тем, что идёт после своего Primary.
#[test]
fn widget_style_and_kvantum_share_one_switch() {
    for de in ["kde", "lxqt", "gnome", "sway"] {
        for tool in ["qt6ct", ""] {
            let kv = kv_plan(de, tool);
            let ws = TH::apply_qt_style_plan(
                "kvantum",
                &TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme: tool, icon_theme: String::new() },
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

// ── 14-ter. LXQt: родные ключи темы GTK, значков и курсора ─────────────────
//
// Ключи сверены со строками установленных бинарей LXQt 2.x:
// lxqt-config-appearance — `icon_theme`, `$XDG_CONFIG_HOME/gtk-%1/settings.ini`,
// `Mouse/cursor_theme`; lxqt-session — `Mouse`/`cursor_theme`.

#[test]
fn lxqt_gets_native_theme_keys() {
    let (_, gtk) = run_plan(&TH::apply_gtk_plan("Dracula", "lxqt", HOME), false);
    assert_eq!(gtk[".config/gtk-3.0/settings.ini"], "[Settings]\ngtk-theme-name=Dracula\n");
    assert_eq!(gtk[".config/gtk-4.0/settings.ini"], "[Settings]\ngtk-theme-name=Dracula\n");

    // Значки: gsettings (для GTK) + свой lxqt.conf; перед ним ForceOk —
    // упавший gsettings (нет dconf) не должен отнимать у LXQt родной ключ.
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

    // Чужие ключи в lxqt.conf сохраняются, значение правится в своей секции.
    let before = "[General]\ntheme=Clearlooks\nicon_theme=breeze\n\n[Qt]\nstyle=Fusion\n";
    assert_eq!(
        crate_defaults::set_ini_value(Some(before), "[General]", "icon_theme", "Papirus"),
        "[General]\ntheme=Clearlooks\nicon_theme=Papirus\n\n[Qt]\nstyle=Fusion\n"
    );

    // Другим окружениям ключи LXQt не достаются.
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

/// COSMIC: значки уходят и в родной ключ libcosmic (панель, cosmic-files),
/// строкой RON в кавычках — как пишет `cosmic-config`; удалась запись —
/// применение удалось, даже если gsettings не ответил. Другим окружениям
/// этого ключа не достаётся.
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

/// Спрашивать про пакет незачем там, где стиль Qt выбирается штатно.
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

/// Deepin: программы DTK рисуются стилем Chameleon, и переменная стиля Qt
/// на весь сеанс отняла бы его. Ни `QT_QPA_PLATFORMTHEME`, ни
/// `QT_STYLE_OVERRIDE` не пишутся, а пустой план честно не даёт «успеха»;
/// раздел в Deepin скрыт со своей причиной.
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
    // Оговорка «после перезахода» в Deepin не звучит — раздела там нет.
    assert!(rexon_core::de_customization::condition_note_id("theme-kvantum", "deepin").is_none());
}

/// Те же сообщения на исходном языке.
#[test]
fn texts_in_english() {
    let _lang = i18n_pin::en();
    assert_eq!(TH::msg_archive_no_dirs(), "The archive contains no folders");
    assert_eq!(TH::msg_extract_failed_prefix(), "Unpacking error: ");
    assert_eq!(TH::msg_install_failed_prefix(), "Couldn't install. Try it manually: ");
}

// ── 15. Курсор во всех окружениях: тема + размер (30.09) ──────────────────
//
// Родные ключи сверены с первоисточниками (см. шапку раздела в themes.rs):
// plasma-apply-cursortheme --help (`--size`), sway-input(5) (`seat … xcursor_theme
// <theme> [<size>]`), вики niri (`cursor { xcursor-theme; xcursor-size }`,
// `include` с 25.11), вики и исходники Hyprland 0.56.2 (`hl.env`, `env =`,
// hyprland.lua прежде hyprland.conf), строки xfsettingsd (`/Gtk/CursorThemeSize`).

fn sized(de: &str) -> Plan {
    TH::apply_cursor_plan_sized("Bibata", Some(32), de, HOME, None, true).unwrap()
}

/// Что получает каждое окружение при выборе темы и размера.
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
            // Plasma сама ставит XCURSOR_* из kcminputrc при входе.
            assert!(env.is_none(), "kde: лишний environment.d");
        } else {
            assert_eq!(env.map(String::as_str), Some("XCURSOR_THEME=Bibata\nXCURSOR_SIZE=32\n"), "{de}");
        }
    }
    let (_, lxqt) = run_plan(&sized("lxqt"), false);
    assert_eq!(lxqt[".config/lxqt/session.conf"], "\n[Mouse]\ncursor_theme=Bibata\ncursor_size=32\n");
    // Без размера размер не трогается нигде.
    let (calls, files) = run_plan(&TH::apply_cursor_plan("Bibata", "gnome", HOME, true).unwrap(), false);
    assert!(!calls.iter().any(|c| c.contains(&"cursor-size".to_string())));
    assert!(!files[".config/gtk-3.0/settings.ini"].contains("size"));
}

/// У тайловых «применено» больше не безусловное: `ok` даёт запись нашего
/// файла окружения, а конфиг композитора правится отдельным шагом.
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
    // Имя, которое нельзя нести в чужой конфиг, туда и не идёт.
    let p = TH::apply_cursor_plan_sized("Bad\"Name", Some(24), "sway", HOME, None, true).unwrap();
    assert!(!p.steps.iter().any(|s| matches!(s, ApplyStep::WmConfig(_))));
}

/// river меняет указатель своей командой сразу, Wayfire — по родному ключу
/// `[input]` в своём конфиге (перечитывает его сам); конфига нет — не заводим.
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
    // Ключ в стиле wayfire.ini (`key = value`) правится по месту, а не
    // дописывается вторым.
    let before = "[core]\nplugins = autostart\n\n[input]\ncursor_theme = default\nxkb_layout = us,ru\n";
    assert_eq!(
        TH::ini_guarded_text(Some(before), "[input]", "cursor_theme", "Bibata", true, false).as_deref(),
        Some("[core]\nplugins = autostart\n\n[input]\ncursor_theme=Bibata\nxkb_layout = us,ru\n")
    );
    assert_eq!(TH::ini_guarded_text(None, "[input]", "cursor_theme", "Bibata", true, false), None);
    // Похожий ключ — не тот же.
    assert_eq!(
        crate_defaults::set_ini_value(Some("[input]\ncursor_theme_x = a\n"), "[input]", "cursor_theme", "B"),
        "[input]\ncursor_theme_x = a\ncursor_theme=B\n"
    );
    // Другим окружениям этих шагов не достаётся.
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
    // sway снимает с аргумента `include` ВСЕ неэкранированные кавычки, а
    // потом режет путь по пробелам wordexp: кавычки wordexp доходят до него
    // только экранированными (`\'`), а сам аргумент — в двойных кавычках.
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
    // Повтор — тот же текст; новое тело — на месте старого.
    assert_eq!(TH::upsert_managed_block(&once, "#", "cursor", "include /a"), once);
    let twice = TH::upsert_managed_block(&once, "#", "cursor", "include /b");
    assert_eq!(twice.matches(">>> Rexon").count(), 1);
    assert!(twice.contains("include /b") && !twice.contains("include /a"));
    // Строки человека ПОСЛЕ блока не теряются.
    let tail = format!("{once}bindsym $mod+q kill\n");
    let upd = TH::upsert_managed_block(&tail, "#", "cursor", "include /c");
    assert!(upd.ends_with("# <<< Rexon: cursor <<<\nbindsym $mod+q kill\n"));
    // Откат возвращает исходник байт в байт; без перевода строки в конце — тоже.
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

/// Сообщение говорит правду о том, где и когда курсор сменится.
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
    // cosmic-comp при входе берёт XCURSOR_THEME, иначе тему `default`.
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

// ── 15-bis. Исполнитель: конфиги композиторов на диске (приватный каталог) ──
//
// Только шаги WmConfig/WriteSetsOk: gsettings, swaymsg и hyprctl из плана здесь
// НЕ запускаются — сверка не должна касаться живого стола.

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
    // Личного конфига нет, есть конфиг i3 — sway читает его; не трогаем.
    std::fs::create_dir_all(format!("{home}/.config/i3")).unwrap();
    std::fs::write(format!("{home}/.config/i3/config"), "bindsym Mod4+q kill\n").unwrap();
    let r = run_wm_steps("Bibata", "sway", &home);
    assert!(r.ok, "environment.d записан — статус правдив");
    assert_eq!(r.wm.as_ref().unwrap().1, O::Foreign);
    assert_eq!(std::fs::read_to_string(format!("{home}/.config/i3/config")).unwrap(), "bindsym Mod4+q kill\n");
    assert!(!std::path::Path::new(&cfg).exists());
    std::fs::remove_dir_all(format!("{home}/.config/i3")).unwrap();

    // Свой конфиг есть: блок в конце, чужое на месте, копия до правки.
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
    // Второй раз — конфиг тот же, блок один.
    run_wm_steps("Bibata", "sway", &home);
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), now);
    // `sway --validate` всё равно поднимает бэкенд wlroots: без сеанса
    // (ни WAYLAND_DISPLAY, ни DISPLAY — сборка в чистом окружении, CI) он
    // падает на захвате seat ещё до чтения конфига, и сверка краснела бы
    // от машины, а не от конфига.
    let has_display = ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    if have_bin("sway") && has_display {
        // Итог принимает сам sway.
        let st = std::process::Command::new("sway").args(["--validate", "--config", &cfg]).output().unwrap();
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn wm_config_executor_niri_and_hyprland() {
    use TH::WmConfigOutcome as O;
    let home = tmp_home("niri");
    // Конфига нет — композитор заведёт его сам; мы его не создаём (иначе он
    // не положит свой с клавишами).
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

        // Проверка niri отвергла итог (сломанный наш файл) — конфиг человека
        // возвращается как был.
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

/// Чипы размера во «Оформлении» — те же числа, что знает ядро курсора.
#[test]
fn cursor_size_chips_match_core() {
    let keys: Vec<u32> = rexon_core::appearance_page::CURSOR_SIZE_KEYS
        .iter()
        .map(|k| k.parse().unwrap())
        .collect();
    assert_eq!(keys, TH::CURSOR_SIZES);
    assert!(TH::CURSOR_SIZES.contains(&TH::CURSOR_DEFAULT_SIZE));
}

/// По-русски — из настоящего `po/ru.po`.
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

/// Стенд целиком: `LiveInstaller::apply_cursor_size` в приватном HOME.
/// Запуск только руками и только в отдельном окружении (приватные HOME,
/// XDG_CONFIG_HOME и шина сеанса — `dbus-run-session`), иначе gsettings
/// и конфиги ушли бы на живой стол:
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

/// Д3 (проверка окружений 02.10.2026): на KDE смена стиля и Kvantum
/// сообщается запущенным программам сигналом StyleChanged (2), а не
/// PaletteChanged (0) — по 0 программа оставалась на прежнем стиле.
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

/// Д4 (проверка окружений 02.10.2026): под Plasma значки и тема GTK доходят
/// до GTK-программ сразу — значки пишутся `kwriteconfig6 --notify` (его
/// слушает модуль `gtkconfig`), тема GTK отдаётся самому `gtkconfig`.
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
    // Вне KDE модуля Plasma нет — и вызова нет.
    for de in ["gnome", "xfce", "lxqt", "sway"] {
        assert!(!argvs(&TH::apply_gtk_plan("Materia-dark", de, HOME))
            .iter()
            .any(|a| a.iter().any(|x| x.contains("GtkConfig"))), "{de}");
    }
}

/// Д2 (проверка окружений 02.10.2026): под qt6ct Qt-программы берут значки
/// из его конфига. Выбор значков пишется туда, если конфиг есть; заведённый
/// Rexon конфиг получает нынешние значки окружения, а не пустоту (живьём было
/// Papirus → hicolor).
#[test]
fn qt6ct_keeps_icon_theme() {
    // План значков: в qt6ct.conf и qt5ct.conf — только если файл уже есть;
    // kdeglobals (программы KDE вне Plasma) — всегда.
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
    // Нет файла — не заводим; есть — правим, чужое не трогаем.
    assert_eq!(TH::ini_guarded_text(None, "[Appearance]", "icon_theme", "Papirus", true, false), None);
    assert_eq!(
        TH::ini_guarded_text(Some("[Appearance]\nstyle=Fusion\nicon_theme=breeze\n"), "[Appearance]", "icon_theme", "Papirus", true, false)
            .as_deref(),
        Some("[Appearance]\nstyle=Fusion\nicon_theme=Papirus\n")
    );

    // Стиль через qt6ct: нынешние значки окружения ложатся, если своих нет.
    let env = TH::KvantumEnv {
        de_id: "gnome",
        home: "/home/t",
        xdg_config_home: "",
        platform_theme: "qt6ct",
        icon_theme: "Papirus".into(),
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
    // Свой выбор в qt6ct человек уже сделал — не перебиваем.
    assert_eq!(
        TH::ini_guarded_text(Some("[Appearance]\nicon_theme=breeze\n"), "[Appearance]", "icon_theme", "Papirus", false, true),
        None
    );
    // На KDE и LXQt qt6ct не при делах — шага нет.
    for de in ["kde", "lxqt"] {
        let env = TH::KvantumEnv { de_id: de, home: "/home/t", xdg_config_home: "", platform_theme: "qt6ct", icon_theme: "Papirus".into() };
        assert!(!TH::apply_qt_style_plan("Fusion", &env).steps.iter().any(|s| matches!(s, ApplyStep::IniSetGuarded { .. })), "{de}");
    }
    // Источник нынешних значков — те же ключи, что пишет план значков.
    assert_eq!(TH::parse_icon_theme_output("'Papirus'\n"), "Papirus");
    match TH::current_icon_theme_source("xfce", "/h", None) {
        TH::IconThemeSource::Cmd(c) => assert_eq!(c.argv[..3], ["xfconf-query", "-c", "xsettings"]),
        other => panic!("{other:?}"),
    }
}

/// Значки во всех окружениях: без dconf (`gsettings` падает) тайловые и LXQt
/// всё равно отдают набор Qt-программам — qt6ct и kdeglobals; программы KDE
/// получают его везде, кроме самой Plasma (там пишет свой шаг) и Deepin.
#[test]
fn icons_reach_qt_apps_without_dconf() {
    let qt6ct = "/H/.config/qt6ct/qt6ct.conf";
    for de in ["sway", "hyprland", "niri", "river", "wayfire", "lxqt"] {
        let plan = TH::apply_icon_plan("Papirus", de, HOME, true).unwrap();
        let force = plan.steps.iter().position(|s| matches!(s, ApplyStep::ForceOk)).expect(de);
        let guarded = plan.steps.iter().position(|s| matches!(s, ApplyStep::IniSetGuarded { .. })).expect(de);
        assert!(force < guarded, "{de}: ForceOk после записей «если ok»");
        // Исполнение с упавшим gsettings и уже настроенным qt6ct.
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
    // Чужое в kdeglobals сохраняется, правится только свой ключ.
    assert_eq!(
        TH::ini_guarded_text(Some("[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=breeze\n"), "[Icons]", "Theme", "Papirus", false, false).as_deref(),
        Some("[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=Papirus\n")
    );
}

// ── Стандартный курсор окружения (Adwaita в GNOME и Cinnamon) ──────────────

/// Стандартная тема роли узнаётся у самой схемы (умолчание + поправки
/// дистрибутива), а не таблицей; где схемы нет — спрашивать нечего.
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

/// Возврат к стандартному курсору: родной ключ сбрасывается, а не пишется
/// именем темы; наши явные указания (settings.ini GTK, environment.d)
/// убираются; имени небезопасной темы нет ни в одной записи. Cinnamon —
/// своей схемой.
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
    // Защита на месте: обычный план для небезопасной темы по-прежнему не строится.
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

// ── Режим программ (color-scheme) вслед за темой GTK ───────────────────────

/// Тёмная ли тема: цвет фона окна из gtk.css (по ссылкам `@имя`), без него —
/// «dark» в имени; цвет важнее имени.
#[test]
fn gtk_theme_is_dark_by_css_then_name() {
    let dracula = "@define-color bg_color #1e1f29;\n@define-color theme_bg_color @bg_color;\n";
    assert!(TH::gtk_theme_is_dark("Dracula", Some(dracula)));
    let adw = "@define-color window_bg_color #222226;\n@define-color theme_bg_color @window_bg_color;\n";
    assert!(TH::gtk_theme_is_dark("adw-gtk3-dark", Some(adw)));
    assert!(!TH::gtk_theme_is_dark("Materia", Some("@define-color theme_bg_color #f9f9f9;\n")));
    // Имя говорит «Dark», а фон окна светлый — решает цвет.
    let royale = "@define-color bg_color #ebe9ed;\n@define-color theme_bg_color @bg_color;\n";
    assert!(!TH::gtk_theme_is_dark("Windows XP Royale Dark", Some(royale)));
    // Стили в ресурсе GTK — по имени.
    let import = "@import url(\"resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained-dark.css\");\n";
    assert!(TH::gtk_theme_is_dark("Adwaita-dark", Some(import)));
    assert!(!TH::gtk_theme_is_dark("Adwaita", None));
    // Петля ссылок не вешает разбор — решает имя.
    assert!(!TH::gtk_theme_is_dark("Loop", Some("@define-color theme_bg_color @a;\n@define-color a @theme_bg_color;\n")));
}

/// Тёмная тема GTK ставит программам `prefer-dark`, светлая — `default`;
/// только там, где тема GTK пишется в схему GNOME, и только после записи
/// темы. Cinnamon, MATE, XFCE, KDE режим ведут сами.
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
    // Pantheon пишет ещё и ключ своей службы —
    // `pantheon_dark_mode_goes_to_elementary_settings_daemon`.
    for de in ["gnome", "budgie", "cosmic", "sway"] {
        assert_eq!(scheme(de, Some(true)), set("prefer-dark"), "{de}");
        assert_eq!(scheme(de, Some(false)), set("default"), "{de}");
        assert!(scheme(de, None).is_empty(), "{de}");
    }
    for de in ["cinnamon-mint", "mate", "xfce", "kde", "deepin"] {
        assert!(scheme(de, Some(true)).is_empty(), "{de}");
    }
    // Прежний план без режима не изменился.
    assert_eq!(TH::apply_gtk_plan_xdg("T", "gnome", HOME, None), TH::apply_gtk_plan_scheme("T", "gnome", HOME, None, None));
}

/// Pantheon: светлый/тёмный режим ведёт служба io.elementary.settings-daemon
/// по СВОЕМУ ключу `io.elementary.settings-daemon.prefers-color-scheme
/// color-scheme` (его пишет «Параметры → Рабочий стол → Оформление»); из него
/// она раздаёт режим в AccountsService — оттуда его берут Gala, панель и
/// программы elementary — и в ключ GNOME. Один ключ GNOME оставлял оболочку
/// светлой, а по расписанию тёмного режима служба переписывала его назад.
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
    // «Светлый» у elementary — `no-preference` (кнопка «По умолчанию»).
    assert!(ifok(false).contains(&elementary("no-preference")));
    // Остальным окружениям ключа elementary не пишем.
    for de in ["gnome", "budgie", "cosmic"] {
        let plan = TH::apply_gtk_plan_scheme("T", de, HOME, None, Some(true));
        assert!(!format!("{:?}", plan.steps).contains("io.elementary"), "{de}");
    }
}
