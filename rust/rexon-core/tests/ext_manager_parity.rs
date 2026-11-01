//! Golden-сверка порта `ext_manager.rs` с оригиналом
//! `rexon/extensions/manager.py`.
//!
//! Эталоны сняты с ЖИВОГО Python по схеме «песочница + журнал побочных
//! вызовов»: одноразовый генератор материализовал деревья расширений в
//! песочнице (HOME → временный каталог, реальный HOME не тронут), прогнал
//! настоящие `_safe_install`/`_safe_uninstall`/`install_from_file`/
//! `_discover` и записал КАЖДЫЙ побочный вызов (shutil.copytree/copy2/
//! rmtree, os.makedirs/symlink/remove, subprocess.run/Popen с argv и
//! timeout, тексты pkexec-скриптов и генерируемых файлов) как op-лог.
//! Снимок ФС в фикстуре хранит листинги в порядке os.scandir/os.listdir,
//! КАК ИХ ВИДЕЛ Python (включая каталоги, прочитанные после его же
//! копирований). tempfile.mkdtemp подменялся детерминированным счётчиком
//! `{base}/{prefix}{N}` — те же пути даёт `TmpAlloc`.
//!
//! План Rust обязан совпасть с op-логом Python 1-в-1 (порядок, argv,
//! таймауты, тексты скриптов/конфигов байт-в-байт).
//!
//! Известное отклонение: текст ошибки битого JSON — деталь после
//! `manifest.json повреждён: ` у serde своя (JSONDecodeError Python
//! не воспроизводится) — сверяется префикс.

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
    // Каталог помощника раскладки — живой ответ этой машины (голден снят
    // там, где пакета не было): `system_plan::retarget_helper_paths`.
    let raw = rexon_core::system_plan::retarget_helper_paths(include_str!(
        "golden/ext_manager_cases.json"
    ));
    serde_json::from_str(&raw).unwrap()
}

/// Окружение кейса, собранное из фикстуры (владеет данными для замыканий).
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
// Планы установки / удаления
// ============================================================================

#[test]
fn safe_install_plans_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let fx = fx();
    // Все расхождения разом: правка планировщика видна по всем случаям.
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
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = fx();
    // Все расхождения разом: правка планировщика видна по всем случаям.
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
// Валидационный конвейер установки (_install_rxn)
// ============================================================================

/// Лексическая нормализация — аналог os.path.realpath для путей без
/// симлинков (в фикстурах их нет).
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
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let fx = fx();
    for (name, case) in fx["rxn_check"].as_object().unwrap() {
        let result = &case["result"];
        if name == "bad_zip" {
            // Распаковка — исполнение (ext_utils/BadZipFile); текст ошибки
            // Python фиксируем как контракт исполнителя.
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
    // Язык — до разбора эталона: от него зависит `--lang` помощника
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
// Чистые функции
// ============================================================================

#[test]
fn pure_helpers_match_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
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
// Перевод
// ============================================================================

/// Английский интерфейс: отказы установки — без русских букв. Русский —
/// прежний: сверки с эталоном Python выше идут на `po/ru.po`.
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

    // Все отказы конвейера на фикстурах Python — по-английски.
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
// Фон экрана входа LightDM — по приветствию, которое стоит
// ============================================================================

/// План установки фона экрана входа при LightDM с заданными файлами
/// настроек LightDM (путь → текст).
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

/// Приветствие узнаётся по файлам LightDM (поздний перекрывает ранний,
/// `[SeatDefaults]` — тоже) и по `Exec=` описания: у Debian имя сеанса —
/// общий `lightdm-greeter`.
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

/// slick-greeter и lightdm-gtk-greeter — их файл настроек (как прежде);
/// lightdm-deepin-greeter — картинка в общий каталог и в учётную запись;
/// приветствие Pantheon своего фона не имеет — ни пароля, ни записи, а
/// объяснение; незнакомое — тоже без записи.
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
