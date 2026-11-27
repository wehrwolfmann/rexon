//! Golden check of the `ext_manager.rs` port against the original
//! `rexon/extensions/manager.py`.
//!
//! The goldens were captured from LIVE Python using the "sandbox + side-call
//! log" scheme: a one-off generator materialized extension trees in a
//! sandbox (HOME → a temporary directory, the real HOME untouched), ran the
//! real `_safe_install`/`_safe_uninstall`/`install_from_file`/
//! `_discover` and recorded EVERY side call (shutil.copytree/copy2/
//! rmtree, os.makedirs/symlink/remove, subprocess.run/Popen with argv and
//! timeout, texts of pkexec scripts and generated files) as an op log.
//! The FS snapshot in the fixture keeps listings in os.scandir/os.listdir order,
//! AS PYTHON SAW THEM (including directories read after its own
//! copies). tempfile.mkdtemp was replaced by a deterministic counter
//! `{base}/{prefix}{N}` — `TmpAlloc` gives the same paths.
//!
//! The Rust plan must match the Python op log 1-to-1 (order, argv,
//! timeouts, script/config texts byte-for-byte).
//!
//! Known deviation: the broken-JSON error text — the detail after
//! `manifest.json повреждён: ` is serde's own (Python's JSONDecodeError
//! is not reproduced) — only the prefix is checked.

use std::collections::HashMap;

use rexon_core::ext_manager::{
    bad_manifest_json_error, check_rexon_compat, discover, find_background,
    find_manifest_root, install_rxn_check, is_safe_sys_name, load_state,
    safe_install_plan, safe_uninstall_plan, save_state_text, stats, ExtEnv,
    ExtInfo, FsSnap, TmpAlloc, ExtStep, lightdm_greeter, lightdm_background_way,
    LightdmBackground, deepin_greeter_background_step,
};
use serde_json::{json, Map, Value};

mod i18n_pin;

fn fx() -> Value {
    // The layout helper directory is this machine's live answer (the golden was captured
    // where the package was absent): `system_plan::retarget_helper_paths`.
    let raw = rexon_core::system_plan::retarget_helper_paths(include_str!(
        "golden/ext_manager_cases.json"
    ));
    serde_json::from_str(&raw).unwrap()
}

/// Case environment built from the fixture (owns the data for the closures).
struct CaseData {
    fs: FsSnap,
    which: HashMap<String, String>,
    ok_argvs: Vec<Vec<String>>,
    home: String,
    helper_path: String,
    helper_family: String,
    stage_helper: bool,
    system_helper: bool,
    xdg: String,
    session: String,
    wayland: bool,
    tmp_base: String,
    ext_dir: String,
    manifest: Map<String, Value>,
}

impl CaseData {
    fn load(case: &Value) -> Self {
        let cfg = &case["cfg"];
        let which = cfg["which"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        v.as_str().map(|s| (k.clone(), s.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ok_argvs = cfg["ok_argvs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|argv| {
                        argv.as_array()
                            .unwrap()
                            .iter()
                            .map(|s| s.as_str().unwrap().to_string())
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        CaseData {
            fs: FsSnap::from_json(&case["snapshot"]),
            which,
            ok_argvs,
            home: case["home"].as_str().unwrap().to_string(),
            helper_path: case["helper_path"].as_str().unwrap().to_string(),
            helper_family: match cfg["detector_family"].as_str() {
                Some("debian") => "debian".to_string(),
                _ => "arch".to_string(),
            },
            stage_helper: cfg["stage_helper"].as_bool().unwrap_or(false),
            system_helper: cfg["system_helper"].as_bool().unwrap_or(false),
            xdg: cfg["xdg"].as_str().unwrap_or("").to_string(),
            session: cfg["session"].as_str().unwrap_or("").to_string(),
            wayland: cfg["wayland_display"].as_bool().unwrap_or(false),
            tmp_base: case["tmp_base"].as_str().unwrap().to_string(),
            ext_dir: case["ext_dir"].as_str().unwrap().to_string(),
            manifest: case["manifest"].as_object().cloned().unwrap_or_default(),
        }
    }

    fn run<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ExtEnv, &ExtInfo) -> R,
    {
        let tmp = TmpAlloc::new(&self.tmp_base);
        let which = |name: &str| self.which.get(name).cloned();
        let run_ok = |argv: &[String]| self.ok_argvs.iter().any(|a| a == argv);
        let env = ExtEnv {
            home: &self.home,
            fs: &self.fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: &self.xdg,
            xdg_session_type: &self.session,
            wayland_display: self.wayland,
            stage_helper_available: self.stage_helper,
            system_helper_path: &self.helper_path,
            system_helper_exists: self.system_helper,
            helper_family: &self.helper_family,
            tmp: &tmp,
        };
        let ext = ExtInfo { dir: &self.ext_dir, manifest: &self.manifest };
        f(&env, &ext)
    }
}

// ============================================================================
// Install / uninstall plans
// ============================================================================

#[test]
fn safe_install_plans_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let fx = fx();
    // All mismatches at once: a planner change shows up across all cases.
    let mut diverged = Vec::new();
    for (name, case) in fx["install"].as_object().unwrap() {
        let data = CaseData::load(case);
        let plan = serde_json::to_value(data.run(safe_install_plan)).unwrap();
        if plan != case["ops"] {
            diverged.push(format!("install/{name}: план ≠ op-лог Python\n  было:  {}\n  стало: {plan}", case["ops"]));
        }
    }
    assert!(diverged.is_empty(), "{}", diverged.join("\n"));
}

#[test]
fn safe_uninstall_plans_match_python() {
    // Language comes before parsing the golden: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = fx();
    // All mismatches at once: a planner change shows up across all cases.
    let mut diverged = Vec::new();
    for (name, case) in fx["uninstall"].as_object().unwrap() {
        let data = CaseData::load(case);
        let plan = serde_json::to_value(data.run(safe_uninstall_plan)).unwrap();
        if plan != case["ops"] {
            diverged.push(format!("uninstall/{name}: план ≠ op-лог Python\n  было:  {}\n  стало: {plan}", case["ops"]));
        }
    }
    assert!(diverged.is_empty(), "{}", diverged.join("\n"));
}

// ============================================================================
// Install validation pipeline (_install_rxn)
// ============================================================================

/// Lexical normalization — the analogue of os.path.realpath for paths without
/// symlinks (the fixtures have none).
fn lexical_realpath(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

#[test]
fn install_rxn_check_matches_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let fx = fx();
    for (name, case) in fx["rxn_check"].as_object().unwrap() {
        let result = &case["result"];
        if name == "bad_zip" {
            // Unpacking is execution (ext_utils/BadZipFile); the Python error text
            // is pinned as the executor's contract.
            assert_eq!(
                result["error"]["message"],
                json!("Файл повреждён или не является архивом .rxn"),
                "bad_zip: текст контракта"
            );
            continue;
        }
        let fs = FsSnap::from_json(&case["snapshot"]);
        let tmp_dir = case["tmp_dir"].as_str().unwrap();
        let extensions_dir = case["extensions_dir"].as_str().unwrap();

        let Some(src_dir) = find_manifest_root(&fs, tmp_dir) else {
            assert_eq!(
                result["error"],
                json!({"kind": "FileNotFoundError",
                       "message": "manifest.json не найден в архиве .rxn"}),
                "{name}: manifest root"
            );
            continue;
        };
        let text = fs
            .read_text(&format!("{src_dir}/manifest.json"))
            .expect("manifest.json в снимке");
        let manifest: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                let err = bad_manifest_json_error(&e.to_string());
                assert_eq!(result["error"]["kind"], json!(err.kind), "{name}");
                assert!(
                    result["error"]["message"]
                        .as_str()
                        .unwrap()
                        .starts_with("manifest.json повреждён: "),
                    "{name}: префикс ошибки JSON"
                );
                assert!(
                    err.message.starts_with("manifest.json повреждён: "),
                    "{name}: префикс ошибки JSON (Rust)"
                );
                continue;
            }
        };
        let dest_exists = |p: &str| fs.exists(p);
        let got = install_rxn_check(
            &fs,
            &src_dir,
            extensions_dir,
            manifest,
            &lexical_realpath,
            &dest_exists,
        );
        match got {
            Ok(check) => {
                let want = &result["ok"];
                assert!(!want.is_null(), "{name}: Python дал ошибку, Rust — ok");
                assert_eq!(json!(check.ext_id), want["ext_id"], "{name}: id");
                assert_eq!(
                    json!(check.ext_type),
                    want["ext_type"],
                    "{name}: type"
                );
                let rewritten = check
                    .rewritten_manifest_text
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                assert_eq!(rewritten, want["rewritten"], "{name}: rewritten");
            }
            Err(e) => {
                assert_eq!(
                    json!({"kind": e.kind, "message": e.message}),
                    result["error"],
                    "{name}: ошибка"
                );
            }
        }
    }
}

// ============================================================================
// discover / stats / state
// ============================================================================

#[test]
fn discover_stats_state_match_python() {
    // Language comes before parsing the golden: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = fx();
    let case = &fx["discover"];
    let fs = FsSnap::from_json(&case["snapshot"]);
    let extensions_dir = case["extensions_dir"].as_str().unwrap();
    let state_file = case["state_file"].as_str().unwrap();

    let state = load_state(fs.read_text(state_file));
    let exts = discover(&fs, extensions_dir, &state);
    let got: Vec<Value> = exts
        .iter()
        .map(|e| json!([e.ext_id(), e.ext_type(), e.enabled]))
        .collect();
    assert_eq!(json!(got), case["exts"], "discover");

    let pairs: Vec<(String, bool)> = exts
        .iter()
        .map(|e| (e.ext_type().to_string(), e.enabled))
        .collect();
    assert_eq!(
        Value::Object(stats(&pairs)),
        case["stats"],
        "get_stats"
    );

    let state_pairs: Vec<(String, bool)> = exts
        .iter()
        .map(|e| (e.ext_id().to_string(), e.enabled))
        .collect();
    assert_eq!(
        json!(save_state_text(&state_pairs)),
        case["state_text"],
        "_save_state текст"
    );
}

// ============================================================================
// Pure functions
// ============================================================================

#[test]
fn pure_helpers_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let fx = fx();
    let pure = &fx["pure"];

    for (name, want) in pure["is_safe_sys_name"].as_object().unwrap() {
        assert_eq!(
            json!(is_safe_sys_name(name)),
            *want,
            "is_safe_sys_name({name:?})"
        );
    }

    for (req, want) in pure["check_rexon_compat"].as_object().unwrap() {
        let mut meta = Map::new();
        meta.insert(
            "rexon_version".to_string(),
            Value::String(req.clone()),
        );
        let got = match check_rexon_compat(&meta) {
            Ok(()) => Value::Null,
            Err(msg) => Value::String(msg),
        };
        assert_eq!(got, *want, "check_rexon_compat({req:?})");
    }

    for (label, case) in pure["find_background"].as_object().unwrap() {
        let fs = FsSnap::from_json(&case["snapshot"]);
        let got = find_background(&fs, case["data_dir"].as_str().unwrap());
        assert_eq!(json!(got), case["result"], "find_background/{label}");
    }

    for (label, case) in pure["find_manifest_root"].as_object().unwrap() {
        let fs = FsSnap::from_json(&case["snapshot"]);
        let got = find_manifest_root(&fs, case["tmp_dir"].as_str().unwrap())
            .map(Value::String)
            .unwrap_or(Value::Null);
        assert_eq!(got, case["result"], "find_manifest_root/{label}");
    }
}

// ============================================================================
// Translation
// ============================================================================

/// English interface: install refusals carry no Russian letters. Russian stays
/// as before: the checks against the Python golden above run on `po/ru.po`.
#[test]
fn english_install_errors_have_no_russian() {
    let _lang = i18n_pin::en();
    assert_eq!(
        bad_manifest_json_error("x").message,
        "manifest.json is damaged: x"
    );
    let mut meta = Map::new();
    meta.insert("rexon_version".into(), Value::String(">=99.0.0".into()));
    let msg = check_rexon_compat(&meta).unwrap_err();
    assert!(msg.starts_with("Requires Rexon >=99.0.0, but "), "{msg}");
    assert!(msg.ends_with(" is installed"), "{msg}");

    // All pipeline refusals on the Python fixtures are in English.
    let fx = fx();
    let mut checked = 0;
    for (name, case) in fx["rxn_check"].as_object().unwrap() {
        let fs = FsSnap::from_json(&case["snapshot"]);
        let Some(src_dir) = find_manifest_root(&fs, case["tmp_dir"].as_str().unwrap()) else {
            continue;
        };
        let Some(text) = fs.read_text(&format!("{src_dir}/manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        let dest_exists = |p: &str| fs.exists(p);
        if let Err(e) = install_rxn_check(
            &fs,
            &src_dir,
            case["extensions_dir"].as_str().unwrap(),
            manifest,
            &lexical_realpath,
            &dest_exists,
        ) {
            assert!(
                !e.message.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
                "{name}: русское в английском отказе: {}",
                e.message
            );
            checked += 1;
        }
    }
    assert!(checked >= 3, "отказов на фикстурах слишком мало: {checked}");
}

// ============================================================================
// LightDM login screen background — by the installed greeter
// ============================================================================

/// Install plan for the login screen background under LightDM with the given LightDM
/// settings files (path → text).
fn lightdm_bg_plan(lightdm_files: &[(&str, &str)]) -> Vec<ExtStep> {
    let mut dirs = serde_json::Map::new();
    let mut files = serde_json::Map::new();
    dirs.insert("/x/ext/data".into(), json!(["background.jpg"]));
    files.insert("/x/ext/data/background.jpg".into(), json!({"size": 200000, "text": null}));
    for (path, text) in lightdm_files {
        let (dir, name) = path.rsplit_once('/').unwrap();
        let list = dirs.entry(dir.to_string()).or_insert_with(|| json!([]));
        list.as_array_mut().unwrap().push(json!(name));
        files.insert(path.to_string(), json!({"size": text.len(), "text": text}));
    }
    let fs = FsSnap::from_json(&json!({"dirs": dirs, "files": files}));
    let tmp = TmpAlloc::new("/tmp/rexon_plan_t");
    let which = |n: &str| (n == "pkexec").then(|| "/usr/bin/pkexec".to_string());
    let run_ok = |argv: &[String]| argv.iter().any(|a| a == "lightdm.service");
    let env = ExtEnv {
        home: "/home/w",
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "",
        xdg_session_type: "x11",
        wayland_display: false,
        stage_helper_available: true,
        system_helper_path: "/usr/lib/rexon-system-helper",
        system_helper_exists: true,
        helper_family: "arch",
        tmp: &tmp,
    };
    let manifest = json!({"id": "login-bg", "name": "Login", "type": "theme-login-screen"});
    let ext = ExtInfo { dir: "/x/ext", manifest: manifest.as_object().unwrap() };
    safe_install_plan(&env, &ext)
}

fn staged_category(steps: &[ExtStep]) -> Option<String> {
    steps.iter().find_map(|s| match s {
        ExtStep::Run { argv, .. } if argv.first().map(String::as_str) == Some("pkexec") => {
            argv.iter().find(|a| a.ends_with("-bg") || a.as_str() == "cosmic-greeter").cloned()
        }
        _ => None,
    })
}

/// The greeter is recognized from the LightDM files (a later one overrides an earlier one,
/// `[SeatDefaults]` too) and from the description's `Exec=`: on Debian the session name is
/// the generic `lightdm-greeter`.
#[test]
fn lightdm_greeter_is_read_from_its_config() {
    let fs = FsSnap::from_json(&json!({
        "dirs": {"/usr/share/lightdm/lightdm.conf.d": ["01_debian.conf"], "/usr/share/xgreeters": ["lightdm-greeter.desktop"]},
        "files": {
            "/usr/share/lightdm/lightdm.conf.d/01_debian.conf": {"size": 1, "text": "[Seat:*]\ngreeter-session=lightdm-greeter\n"},
            "/usr/share/xgreeters/lightdm-greeter.desktop": {"size": 1, "text": "[Desktop Entry]\nExec=/usr/bin/io.elementary.greeter --x\n"},
            "/etc/lightdm/lightdm.conf": {"size": 1, "text": "[LightDM]\ngreeter-session=ignored\n[Seat:*]\n#greeter-session=x\n"}
        }
    }));
    assert_eq!(lightdm_greeter(&fs), "io.elementary.greeter");
    let over = FsSnap::from_json(&json!({
        "dirs": {"/usr/share/lightdm/lightdm.conf.d": ["50-x.conf"]},
        "files": {
            "/usr/share/lightdm/lightdm.conf.d/50-x.conf": {"size": 1, "text": "[Seat:*]\ngreeter-session=lightdm-gtk-greeter\n"},
            "/etc/lightdm/lightdm.conf": {"size": 1, "text": "[SeatDefaults]\ngreeter-session = lightdm-deepin-greeter\n"}
        }
    }));
    assert_eq!(lightdm_greeter(&over), "lightdm-deepin-greeter");
    assert_eq!(lightdm_greeter(&FsSnap::from_json(&json!({}))), "");
    assert_eq!(lightdm_background_way("slick-greeter"), LightdmBackground::GreeterConfig);
    assert_eq!(lightdm_background_way(""), LightdmBackground::GreeterConfig, "не узнали — как прежде");
    assert_eq!(lightdm_background_way("io.elementary.greeter"), LightdmBackground::UserWallpaper);
    assert_eq!(lightdm_background_way("lightdm-deepin-greeter"), LightdmBackground::DeepinAccount);
    assert_eq!(lightdm_background_way("lightdm-webkit2-greeter"), LightdmBackground::Unknown);
}

/// slick-greeter and lightdm-gtk-greeter — their settings file (as before);
/// lightdm-deepin-greeter — the picture into the shared directory and the user account;
/// the Pantheon greeter has no background of its own — no password, no write, just an
/// explanation; an unknown one — no write either.
#[test]
fn lightdm_background_follows_the_greeter() {
    let _lang = i18n_pin::ru();
    let conf = |g: &str| format!("[Seat:*]\ngreeter-session={g}\n");
    let slick = lightdm_bg_plan(&[("/etc/lightdm/lightdm.conf", &conf("slick-greeter"))]);
    assert_eq!(staged_category(&slick).as_deref(), Some("lightdm-bg"));
    let unset = lightdm_bg_plan(&[]);
    assert_eq!(staged_category(&unset).as_deref(), Some("lightdm-bg"));

    let deepin = lightdm_bg_plan(&[("/etc/lightdm/lightdm.conf", &conf("lightdm-deepin-greeter"))]);
    assert_eq!(staged_category(&deepin).as_deref(), Some("login-bg"));
    let account = deepin.iter().position(|s| *s == deepin_greeter_background_step()).expect("в учётную запись");
    let helper = deepin.iter().position(|s| matches!(s, ExtStep::Run { argv, .. } if argv[0] == "pkexec")).unwrap();
    assert!(helper < account, "сначала картинка в общий каталог");
    let ExtStep::RunNoted { argv, .. } = &deepin[account] else { unreachable!() };
    assert!(argv[2].contains("SetGreeterBackground") && argv[2].contains("--system"), "{argv:?}");

    let pantheon = lightdm_bg_plan(&[("/etc/lightdm/lightdm.conf", &conf("io.elementary.greeter"))]);
    assert_eq!(staged_category(&pantheon), None, "{pantheon:?}");
    assert!(!pantheon.iter().any(|s| matches!(s, ExtStep::Stage { .. })));
    assert!(pantheon.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains("Pantheon"))));

    let web = lightdm_bg_plan(&[("/etc/lightdm/lightdm.conf", &conf("lightdm-webkit2-greeter"))]);
    assert_eq!(staged_category(&web), None);
    assert!(web.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains("lightdm-webkit2-greeter"))));
}

// ============================================================================
// The package does not install the shared `hicolor` icon set
// ============================================================================

/// An icon (cursor) package with `hicolor`/`locolor` subfolders next to its own
/// theme: the theme is installed, the shared sets are not (a note), even when their
/// directories do not exist in the profile yet. Otherwise the package's `index.theme` would override
/// the system one and hide the icons of the whole desktop (defect of 09.11.2026).
#[test]
fn rxn_never_installs_shared_hicolor() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    for kind in ["theme-icons", "theme-cursors"] {
        let fs = FsSnap::from_json(&json!({
            "dirs": {
                "/x/ext/data": ["MyIcons", "hicolor", "locolor"],
                "/x/ext/data/MyIcons": ["index.theme"],
                "/x/ext/data/hicolor": ["index.theme"],
                "/x/ext/data/locolor": [],
            },
            "files": {
                "/x/ext/data/MyIcons/index.theme": {"size": 30, "text": "[Icon Theme]\nName=My\nDirectories=apps\n"},
                "/x/ext/data/hicolor/index.theme": {"size": 40, "text": "[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps\n"},
            },
        }));
        let tmp = TmpAlloc::new("/tmp/rexon_plan_t");
        let which = |_: &str| None;
        let run_ok = |_: &[String]| false;
        let env = ExtEnv {
            home: "/home/w",
            fs: &fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: "",
            xdg_session_type: "wayland",
            wayland_display: true,
            stage_helper_available: false,
            system_helper_path: "/usr/lib/rexon-system-helper",
            system_helper_exists: false,
            helper_family: "arch",
            tmp: &tmp,
        };
        let manifest = json!({"id": "my-icons", "name": "My", "type": kind});
        let ext = ExtInfo { dir: "/x/ext", manifest: manifest.as_object().unwrap() };
        let steps = safe_install_plan(&env, &ext);
        let dests: Vec<&str> = steps
            .iter()
            .filter_map(|s| match s {
                ExtStep::CopyTree { dest, .. } => Some(dest.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(dests, ["/home/w/.local/share/icons/MyIcons"], "{kind}: {steps:?}");
        let notes: Vec<&str> = steps
            .iter()
            .filter_map(|s| match s {
                ExtStep::Notice { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 2, "{kind}: {notes:?}");
        assert!(notes.iter().all(|n| n.contains("shared icon set of all programs")), "{notes:?}");
    }
}
