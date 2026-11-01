//! Golden-сверка порта `system_plan.rs` («планы команд» исполняющего слоя)
//! с оригиналом `rexon/system.py`.
//!
//! Эталоны сняты с самого Python (генератор импортирует живой `rexon.system`,
//! подменяет safe_run/subprocess.run/tempfile.mkstemp/os.remove/
//! `_wait_for_pkg_lock`/`ensure_gentoo_overlay`/наличие helper'ов на фикстуры
//! и ЗАПИСЫВАЕТ реальные вызовы — argv, timeout, env LANG=C):
//!   * `golden/system_plan_texts.json` — тексты helper-скриптов
//!     (rexon-de-helper / rexon-pkg-helper / rexon-stage-helper), policy XML,
//!     polkit rules, таблицы команд per family (+ тексты RuntimeError),
//!     LOCK_PATHS, privileged_pkg_cmd / privileged_stage_cmd,
//!     de_helper_supports, NixOS-сниппеты, Gentoo overlays, DM-константы —
//!     сверка текст-в-текст.
//!   * `golden/system_plan_batch_cases.json` — трейсы update_db / install /
//!     remove / install_batch (+ тексты wrapper-скриптов с chown-граблей
//!     fs.protected_regular, прочитанные из записанных Python tmp-файлов) и
//!     DE-оркестрация: dm-enable (через install_de с моками), remove_de
//!     (2-уровневая защита), switch_de guard, logout_session по окружениям.
//!
//! План Rust обязан совпасть с записанным трейсом Python 1-в-1 по argv/
//! timeout/env. ЕДИНСТВЕННОЕ исключение — фаза транзитивного замыкания в
//! remove: порядок -Qi там в Python недетерминирован (итерация set при
//! рандомизированных hash), поэтому генератор отсортировал этот сегмент, и
//! тест сортирует такой же сегмент плана (множество вызовов на каждом проходе
//! замыкания детерминировано — см. комментарий в remove_plan).

mod i18n_pin;

use rexon_core::system::{DistroDetector, PkgFamily};
use rexon_core::system_plan::{
    de_helper_pkg_table, de_helper_script, de_helper_supports, de_policy_xml,
    dm_enable_step, install_batch_plan, install_cmd, install_plan, lock_paths,
    logout_cmds, pkg_helper_script, pkg_manager_name, pkg_policy_xml,
    privileged_pkg_cmd, privileged_stage_cmd, query_cmd, remove_cmd,
    remove_de_decision, remove_plan, stage_helper_script,
    stage_policy_xml, switch_de_guard, update_db_cmd, update_db_plan, Cmd,
    PlanStep, RemoveDeDecision, DE_HELPER_DISPLAY_MANAGERS, DE_HELPER_FAMILIES,
    DE_HELPER_PATH, DE_INSTALL_ACTION, DE_POLICY_PATH, DM_FORCE_ENABLE,
    DM_PKG_FOR_UNIT, GENTOO_OVERLAYS, MATE_LAYOUTS, NIXOS_DE_SNIPPETS, nixos_de_snippet,
    PKG_ACTION, PKG_HELPER_FAMILIES, PKG_HELPER_OPS, PKG_HELPER_PATH,
    PKG_POLICY_PATH, REXON_POLKIT_RULES_PATH, STAGE_ACTION, STAGE_CATEGORIES,
    STAGE_DIR_ROOT, STAGE_HELPER_PATH, STAGE_POLICY_PATH,
};
use serde_json::{json, Map, Value};

fn texts_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_plan_texts.json")).unwrap()
}

/// Тот же эталон, но с путём помощника ЭТОЙ машины — для сверок КОМАНД.
///
/// Каталога у помощников два, и оба законны: голден снят с машины без
/// пакета и держит `/usr/local/lib`, а там, где пакет установлен, движок
/// зовёт помощника из `/usr/lib`. Пин при этом продолжает ловить и сырой
/// `pkexec` вместо помощника, и любую правку аргументов — подробности в
/// `system_plan::retarget_helper_paths`.
///
/// ⚠️ НЕ для `constants_match_python` и `scripts_match_python_verbatim`:
/// там пин про константы `*_HELPER_PATH` и про XML политики, которые
/// установщик пишет ровно в `/usr/local/lib`, — им замена вредна.
fn texts_fx_live_helpers() -> Value {
    fx_retargeted(include_str!("golden/system_plan_texts.json"))
}

/// Разобрать эталон, подставив в него путь помощника ЭТОЙ машины.
fn fx_retargeted(raw: &str) -> Value {
    serde_json::from_str(&rexon_core::system_plan::retarget_helper_paths(raw)).unwrap()
}

fn batch_fx() -> Value {
    fx_retargeted(include_str!("golden/system_plan_batch_cases.json"))
}

fn fam(v: &Value) -> PkgFamily {
    PkgFamily::from_value(v.as_str().unwrap()).unwrap()
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn opt_argv(v: Option<Vec<String>>) -> Value {
    match v {
        Some(a) => json!(a),
        None => Value::Null,
    }
}

/// План → трейс в формате эталона ({kind:"run",argv,timeout,lang_c} /
/// {kind:"wait_lock"} / {kind:"ensure_overlay",overlay}); файловые шаги
/// (WriteFile/RemoveFiles) в safe_run-трейс Python не попадают.
fn plan_trace(steps: &[PlanStep]) -> Vec<Value> {
    let mut out = Vec::new();
    for st in steps {
        match st {
            PlanStep::Run { cmd, .. } => out.push(json!({
                "kind": "run",
                "argv": cmd.argv,
                "timeout": cmd.timeout_secs,
                "lang_c": cmd.lang_c,
            })),
            PlanStep::WaitPkgLock { .. } => out.push(json!({"kind": "wait_lock"})),
            PlanStep::EnsureGentooOverlay { overlay } => out.push(json!({
                "kind": "ensure_overlay", "overlay": overlay,
            })),
            PlanStep::WriteFile { .. } | PlanStep::RemoveFiles { .. } => {}
        }
    }
    out
}

/// Эталонный трейс без служебных полей (phase/overlay у wait/run).
fn fixture_trace(v: &Value) -> Vec<Value> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let kind = c["kind"].as_str().unwrap();
            match kind {
                "run" => json!({
                    "kind": "run",
                    "argv": c["argv"],
                    "timeout": c["timeout"],
                    "lang_c": c["lang_c"],
                }),
                "ensure_overlay" => json!({
                    "kind": "ensure_overlay", "overlay": c["overlay"],
                }),
                _ => json!({"kind": kind}),
            }
        })
        .collect()
}

fn output_map(fixture: &Value) -> impl Fn(&str) -> Option<String> + '_ {
    let map = fixture.as_object().expect("объект pkg→stdout");
    move |pkg: &str| map.get(pkg).and_then(Value::as_str).map(String::from)
}

// ============================================================================
// Тексты и таблицы
// ============================================================================

#[test]
fn constants_match_python() {
    let fx = texts_fx();
    let c = &fx["constants"];
    assert_eq!(json!(DE_INSTALL_ACTION), c["de_install_action"]);
    assert_eq!(json!(DE_HELPER_PATH), c["de_helper_path"]);
    assert_eq!(json!(DE_POLICY_PATH), c["de_policy_path"]);
    assert_eq!(json!(REXON_POLKIT_RULES_PATH), c["rexon_polkit_rules_path"]);
    assert_eq!(json!(PKG_ACTION), c["pkg_action"]);
    assert_eq!(json!(PKG_HELPER_PATH), c["pkg_helper_path"]);
    assert_eq!(json!(PKG_POLICY_PATH), c["pkg_policy_path"]);
    assert_eq!(json!(STAGE_ACTION), c["stage_action"]);
    assert_eq!(json!(STAGE_HELPER_PATH), c["stage_helper_path"]);
    assert_eq!(json!(STAGE_POLICY_PATH), c["stage_policy_path"]);
    assert_eq!(json!(DE_HELPER_FAMILIES), c["de_helper_families"]);
    assert_eq!(json!(PKG_HELPER_FAMILIES), c["pkg_helper_families"]);
    assert_eq!(json!(PKG_HELPER_OPS), c["pkg_helper_ops"]);
    assert_eq!(
        json!(DE_HELPER_DISPLAY_MANAGERS),
        c["de_helper_display_managers"]
    );
    assert_eq!(json!(DM_FORCE_ENABLE), c["dm_force_enable"]);
    assert_eq!(json!(STAGE_CATEGORIES), c["stage_categories"]);
    assert_eq!(json!(MATE_LAYOUTS), c["mate_layouts"]);

    let mut m = Map::new();
    for (k, v) in DM_PKG_FOR_UNIT {
        m.insert((*k).to_string(), Value::from(*v));
    }
    assert_eq!(Value::Object(m), c["dm_pkg_for_unit"]);
    let mut m = Map::new();
    for (k, v) in STAGE_DIR_ROOT {
        m.insert((*k).to_string(), Value::from(*v));
    }
    assert_eq!(Value::Object(m), c["stage_dir_root"]);
}

#[test]
fn scripts_match_python_verbatim() {
    let fx = texts_fx();
    let s = &fx["scripts"];
    // ⛔ ОСОЗНАННОЕ РАСХОЖДЕНИЕ С ЖИВЫМ PYTHON (25.09.2026): эталоны
    // `pkg_helper_script` и `de_helper_script` пересняты после починки дыры
    // в `org.rexon.pkg` (имя пакета вместо пути, семейство по машине,
    // команда массивом вместо `eval`). Байт-паритет с прежним Python здесь
    // означал бы «верни дыру», поэтому пин теперь стоит на ИСПРАВЛЕННОМ
    // тексте: он по-прежнему ловит любую правку помощника, но не тянет
    // назад. Генератор правила «YES на всё org.rexon.*» удалён совсем —
    // ключа `rexon_polkit_rules_js` в эталоне больше нет.
    // Переснято 30.09.2026 (де-, pkg- и stage-помощник): сообщения
    // помощников переведены (решение Wehrwolfmann) — текст для человека
    // идёт ключом из таблицы, вшитой из каталога (`system_l10n`), язык —
    // аргументом `--lang` из белого списка po/LINGUAS. Проверки, белые
    // списки и пути не менялись.
    // Переснято 28.10.2026 (pkg-помощник): операции пересборки build-deps и
    // install-built — один пароль на установку «Deepin на Treeland».
    // Переснято 28.10.2026 (pkg-помощник): вечное по семействам (ядра,
    // драйверы, прошивки), отказ снять пакеты текущего стола и активного
    // экрана входа (помощник узнаёт их сам: ссылка display-manager.service,
    // сеансы logind), установка только из белого списка таблиц ядра.
    // Переснято 31.10.2026 (pkg-помощник): в белом списке Arch — compiz,
    // пакет образа «Как в Garuda» (эффекты окон на MATE); список пакетов
    // образов вливается туда сам (`distro_looks::builtin_package_names`).
    assert_eq!(
        json!(de_helper_pkg_table()),
        s["de_helper_pkg_table"],
        "de-helper pkg table"
    );
    assert_eq!(json!(i18n_pin::ru_only(&de_helper_script())), s["de_helper_script"], "de-helper");
    assert_eq!(json!(i18n_pin::ru_only(&de_policy_xml())), s["de_policy_xml"], "de policy");
    assert_eq!(json!(i18n_pin::ru_only(&pkg_helper_script())), s["pkg_helper_script"], "pkg-helper");
    assert_eq!(json!(i18n_pin::ru_only(&pkg_policy_xml())), s["pkg_policy_xml"], "pkg policy");
    assert_eq!(
        json!(i18n_pin::ru_only(&stage_helper_script())),
        s["stage_helper_script"],
        "stage-helper"
    );
    assert_eq!(json!(i18n_pin::ru_only(&stage_policy_xml())), s["stage_policy_xml"], "stage policy");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn cmd_tables_match_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = texts_fx();
    let t = &fx["cmd_tables"];
    let pkgs = vec!["pkg-a".to_string(), "pkg-b".to_string()];
    for f in [
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
    ] {
        let key = f.as_str();
        let check = |got: Result<Vec<String>, String>, want: &Value, what: &str| {
            match got {
                Ok(argv) => assert_eq!(json!(argv), *want, "{key}: {what}"),
                Err(e) => assert_eq!(json!(e), want["error"], "{key}: {what} err"),
            }
        };
        check(install_cmd(f, &pkgs), &t["install"][key], "install");
        check(remove_cmd(f, &pkgs), &t["remove"][key], "remove");
        check(query_cmd(f, "pkg-a"), &t["query"][key], "query");
        assert_eq!(
            match update_db_cmd(f) {
                Some(argv) => json!(argv),
                None => Value::Null,
            },
            t["update_db"][key],
            "{key}: update_db"
        );
        assert_eq!(
            json!(pkg_manager_name(f, None)),
            t["pkg_manager_name"][key],
            "{key}: name"
        );
    }
    assert_eq!(
        json!(pkg_manager_name(PkgFamily::Arch, Some("yay"))),
        t["pkg_manager_name"]["arch+yay"],
        "имя с AUR-хелпером"
    );

    // LOCK_PATHS.
    for (key, want) in fx["lock_paths"].as_object().unwrap() {
        let f = PkgFamily::from_value(key).unwrap();
        assert_eq!(json!(lock_paths(f)), *want, "{key}: lock_paths");
    }
    for f in [PkgFamily::Nix, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown] {
        assert!(lock_paths(f).is_empty(), "{}: лока нет", f.as_str());
    }

    // Таблицы install_batch.
    let mut m = Map::new();
    // Приписка «# + …» к сниппету — подсказка человеку, она из каталога
    // перевода; эталон русский, язык закреплён в начале сверки.
    for (k, _) in NIXOS_DE_SNIPPETS {
        m.insert((*k).to_string(), Value::from(nixos_de_snippet(k).unwrap()));
    }
    assert_eq!(Value::Object(m), fx["nixos_de_snippets"], "NIXOS_DE_SNIPPETS");
    let mut m = Map::new();
    for (k, v) in GENTOO_OVERLAYS {
        m.insert((*k).to_string(), Value::from(*v));
    }
    assert_eq!(Value::Object(m), fx["gentoo_overlays"], "GENTOO_OVERLAYS");
}

#[test]
fn de_helper_supports_matches_python() {
    let fx = texts_fx();
    for (key, want) in fx["de_helper_supports"].as_object().unwrap() {
        let (de, family) = key.split_once('|').unwrap();
        assert_eq!(
            json!(de_helper_supports(de, family)),
            *want,
            "supports({de}, {family})"
        );
    }
}

#[test]
fn privileged_pkg_cmd_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = texts_fx_live_helpers();
    for case in fx["privileged_pkg_cmd"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap())
            .unwrap_or(PkgFamily::Unknown);
        let packages: Vec<String> = match case["packages"].as_array() {
            Some(a) => a.iter().map(|s| s.as_str().unwrap().to_string()).collect(),
            None => Vec::new(),
        };
        let got = privileged_pkg_cmd(
            family,
            case["op"].as_str().unwrap(),
            &packages,
            case["helper_available"].as_bool().unwrap(),
        );
        assert_eq!(opt_argv(got), case["expected"], "{label}");
    }
}

#[test]
fn privileged_stage_cmd_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = texts_fx_live_helpers();
    // __STAGED__ — плейсхолдер реального mkdtemp-каталога из генератора.
    let staged_real = "/tmp/rexon_stage_golden_x";
    for case in fx["privileged_stage_cmd"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let subst = |v: &Value| -> String {
            v.as_str().unwrap().replace("__STAGED__", staged_real)
        };
        let staged = subst(&case["staged"]);
        let realpath = case["staged_realpath"].as_str().map(|r| {
            r.replace("__STAGED__", staged_real)
        });
        let got = privileged_stage_cmd(
            case["category"].as_str().unwrap(),
            case["name"].as_str().unwrap(),
            &staged,
            case["staged_is_dir"].as_bool().unwrap(),
            realpath.as_deref(),
            case["helper_available"].as_bool().unwrap(),
        );
        let want: Value = match case["expected"].as_array() {
            Some(a) => json!(a
                .iter()
                .map(|x| x.as_str().unwrap().replace("__STAGED__", staged_real))
                .collect::<Vec<_>>()),
            None => Value::Null,
        };
        assert_eq!(opt_argv(got), want, "{label}");
    }
}

// ============================================================================
// Планы update_db / install / remove
// ============================================================================

#[test]
fn update_db_plan_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["update_db"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let steps = update_db_plan(
            fam(&case["family"]),
            case["pkg_helper_available"].as_bool().unwrap(),
        );
        assert_eq!(
            json!(plan_trace(&steps)),
            json!(fixture_trace(&case["trace"])),
            "{label}: трейс"
        );
        assert!(case["ok"].as_bool().unwrap(), "{label}: оракул ok");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn install_plan_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["install"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let packages = str_vec(&case["packages"]);
        let plan = install_plan(
            fam(&case["family"]),
            &packages,
            case["aur"].as_bool().unwrap(),
            case["aur_helper"].as_str(),
            case["pkg_helper_available"].as_bool().unwrap(),
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(
            json!(plan_trace(&plan.steps)),
            json!(fixture_trace(&case["trace"])),
            "{label}: трейс"
        );
        let ok = case["ok"].as_bool().unwrap();
        let message = case["message"].as_str().unwrap();
        if let Some((eok, emsg)) = &plan.early_result {
            assert_eq!((*eok, emsg.as_str()), (ok, message), "{label}: early");
        } else if !ok {
            // Отказ pkexec: Python строит подсказку sudo — она же в плане.
            assert_eq!(plan.sudo_hint, message, "{label}: sudo hint");
        }
    }
}

#[test]
fn remove_plan_matches_python() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["remove"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let packages = str_vec(&case["packages"]);
        let qg = output_map(&case["qg"]);
        let qi = output_map(&case["qi"]);
        let installed: Vec<String> = str_vec(&case["installed"]);
        let query = |pkg: &str| installed.iter().any(|p| p == pkg);
        let plan = remove_plan(
            fam(&case["family"]),
            &packages,
            &qg,
            &qi,
            &query,
            case["pkg_helper_available"].as_bool().unwrap(),
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));

        // Пофазная сверка: closure-сегмент сортируется с обеих сторон
        // (недетерминизм итерации set в Python — см. заголовок).
        let mut got: Vec<(String, Value)> = Vec::new();
        for st in &plan.steps {
            match st {
                PlanStep::Run { cmd, description, .. } => got.push((
                    description.clone(),
                    json!({"argv": cmd.argv, "timeout": cmd.timeout_secs,
                           "lang_c": cmd.lang_c}),
                )),
                PlanStep::WaitPkgLock { .. } => {
                    got.push(("wait_lock".to_string(), Value::Null))
                }
                _ => {}
            }
        }
        let mut want: Vec<(String, Value)> = case["trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let phase = c["phase"].as_str().unwrap().to_string();
                if c["kind"] == "run" {
                    (phase, json!({"argv": c["argv"], "timeout": c["timeout"],
                                   "lang_c": c["lang_c"]}))
                } else {
                    (phase, Value::Null)
                }
            })
            .collect();
        let sort_closure = |v: &mut Vec<(String, Value)>| {
            let idx: Vec<usize> = v
                .iter()
                .enumerate()
                .filter(|(_, (p, _))| p == "closure")
                .map(|(i, _)| i)
                .collect();
            let mut seg: Vec<(String, Value)> =
                idx.iter().map(|&i| v[i].clone()).collect();
            seg.sort_by_key(|(_, val)| val["argv"].to_string());
            for (&i, item) in idx.iter().zip(seg) {
                v[i] = item;
            }
        };
        sort_closure(&mut got);
        sort_closure(&mut want);
        assert_eq!(json!(got), json!(want), "{label}: трейс");

        assert_eq!(json!(plan.safe), case["safe"], "{label}: safe");
        assert_eq!(json!(plan.skipped), case["skipped"], "{label}: skipped");

        let ok = case["ok"].as_bool().unwrap();
        let message = case["message"].as_str().unwrap();
        if let Some((eok, emsg)) = &plan.result_without_exec {
            assert_eq!((*eok, emsg.as_str()), (ok, message), "{label}: результат");
        } else if ok {
            // Успех: message = stdout мока ("ok") + success_note.
            assert_eq!(
                format!("ok{}", plan.success_note),
                message,
                "{label}: note"
            );
        } else {
            assert_eq!(plan.sudo_hint, message, "{label}: sudo hint");
        }
    }
}

// ============================================================================
// install_batch
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn install_batch_plan_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    // Переснято 01.10.2026 (оба SteamOS-случая): скрипт установки на SteamOS
    // получил `export LC_ALL=C LANG=C`, как у обычного пути, — его вывод
    // pacman разбирает парсер хода установки.
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["install_batch"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let mut info = Map::new();
        for (k, v) in case["detector"].as_object().unwrap() {
            info.insert(k.clone(), v.clone());
        }
        let detector = DistroDetector { info };
        let steamos = detector.is_steamos();
        let (tmp_sh, tmp_log) = if steamos {
            ("/tmp/rexon_steamos_TEST.sh", "/tmp/rexon_steamos_TEST.log")
        } else {
            ("/tmp/rexon_install_TEST.sh", "/tmp/rexon_install_TEST.log")
        };
        let packages = str_vec(&case["packages"]);
        let aur = str_vec(&case["aur_packages"]);
        let plan = install_batch_plan(
            fam(&case["family"]),
            &detector,
            &packages,
            &aur,
            case["aur_helper"].as_str(),
            case["update_first"].as_bool().unwrap(),
            case["de_id"].as_str(),
            case["de_helper_available"].as_bool().unwrap(),
            tmp_sh,
            tmp_log,
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));

        assert_eq!(
            json!(plan_trace(&plan.steps)),
            json!(fixture_trace(&case["trace"])),
            "{label}: трейс"
        );
        assert_eq!(
            plan.used_de_helper,
            case["used_de_helper"].as_bool().unwrap(),
            "{label}: used_de_helper"
        );

        let ok = case["ok"].as_bool().unwrap();
        let message = case["message"].as_str().unwrap();
        match &plan.early_result {
            Some((eok, emsg)) => {
                assert_eq!((*eok, emsg.as_str()), (ok, message), "{label}: early");
            }
            None => {
                assert!(ok, "{label}: оракул ok");
                assert_eq!(plan.final_message, message, "{label}: сообщение");
            }
        }

        // Тексты записанных файлов (wrapper с chown-граблей) и удаление.
        let files = case["files"].as_object().unwrap();
        let mut planned_files: Map<String, Value> = Map::new();
        let mut planned_removed: Vec<String> = Vec::new();
        for st in &plan.steps {
            match st {
                PlanStep::WriteFile { path, content, .. } => {
                    let base = path.rsplit('/').next().unwrap().to_string();
                    planned_files.insert(base, Value::from(content.clone()));
                }
                PlanStep::RemoveFiles { paths } => {
                    planned_removed.extend(paths.iter().cloned());
                }
                _ => {}
            }
        }
        assert_eq!(
            Value::Object(planned_files),
            Value::Object(files.clone()),
            "{label}: файлы/скрипты"
        );
        assert_eq!(
            json!(planned_removed),
            case["removed_files"],
            "{label}: удаление tmp"
        );
    }
}

// ============================================================================
// DE-оркестрация
// ============================================================================

#[test]
fn dm_enable_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["dm_enable"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let installed = str_vec(&case["installed_pkgs"]);
        let got = dm_enable_step(
            fam(&case["family"]),
            &installed,
            case["used_de_helper"].as_bool().unwrap(),
            case["pkg_helper_available"].as_bool().unwrap(),
            // Голдены сняты до появления правила «чужой DM не трогаем»:
            // в них экрана входа нет, поэтому dm_present по умолчанию false.
            case["dm_present"].as_bool().unwrap_or(false),
        );
        let want = case["dm_calls"].as_array().unwrap();
        match got {
            None => assert!(want.is_empty(), "{label}: DM не ожидался"),
            Some((_unit, Cmd { argv, timeout_secs, .. })) => {
                assert_eq!(want.len(), 1, "{label}: ровно один вызов");
                assert_eq!(json!(argv), want[0]["argv"], "{label}: argv");
                assert_eq!(json!(timeout_secs), want[0]["timeout"], "{label}: timeout");
            }
        }
    }
}

#[test]
fn remove_de_guard_matches_python() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["remove_de"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let packages = str_vec(&case["packages"]);
        let aur = str_vec(&case["aur"]);
        let got = remove_de_decision(
            case["de"].as_str().unwrap(),
            case["display_name"].as_str().unwrap(),
            case["current_de"].as_str().unwrap(),
            case["force"].as_bool().unwrap(),
            &packages,
            &aur,
        );
        let ok = case["ok"].as_bool().unwrap();
        match got {
            RemoveDeDecision::Refused(msg) => {
                assert!(!ok, "{label}: оракул прошёл, порт отказал");
                assert_eq!(json!(msg), case["message"], "{label}: текст отказа");
                assert!(
                    case["remove_called_with"].as_array().unwrap().is_empty(),
                    "{label}: remove не должен был вызываться"
                );
            }
            RemoveDeDecision::Proceed(all) => {
                assert!(ok, "{label}: оракул отказал, порт прошёл");
                assert_eq!(
                    json!(vec![all]),
                    case["remove_called_with"],
                    "{label}: пакеты remove (основные + AUR)"
                );
            }
        }
    }
}

#[test]
fn switch_de_guard_matches_python() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["switch_de_guard"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let got = switch_de_guard(
            case["current"].as_str().unwrap(),
            case["target"].as_str().unwrap(),
            case["target_display"].as_str().unwrap(),
        );
        assert!(!case["ok"].as_bool().unwrap(), "{label}: guard-кейс");
        assert_eq!(
            json!(got.expect("guard должен сработать")),
            case["message"],
            "{label}: текст"
        );
    }
}

#[test]
fn logout_cmds_match_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = batch_fx();
    for case in fx["logout"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let env = &case["env"];
        let got = logout_cmds(
            env["XDG_CURRENT_DESKTOP"].as_str().unwrap(),
            env["DESKTOP_SESSION"].as_str().unwrap(),
            env["XDG_SESSION_ID"].as_str().unwrap(),
        );
        let got_json: Vec<Value> = got
            .iter()
            .map(|c| json!({"argv": c.argv, "timeout": c.timeout_secs}))
            .collect();
        assert_eq!(json!(got_json), case["cmds"], "{label}: команды логаута");
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Recovery-часть порта: версия пакета в репозиториях, откат форков Plasma,
// Plymouth, SDDM.
//
// Эталоны — `golden/system_recovery_cases.json`, снятые с ЖИВОГО
// `rexon.system` при выключенном мосте (Python-путь до его удаления, оракул —
// Python-эталоны `_py_*`). Сырьё живое, с этой машины (CachyOS/KDE):
//   * `pacman -Si plasma-desktop` / `gnome-shell` (epoch `1:50.3-1.1`) /
//     несуществующий пакет; `pacman -Qi plasma-desktop`; `pacman -Qq`;
//   * реальный список /usr/share/plymouth/themes (13 тем, включая брендинг
//     cachyos-bootanimation) и реальный /usr/share/plymouth/plymouthd.defaults;
//   * реальные /etc/sddm.conf.d (kde_settings.conf) и
//     /usr/lib/sddm/sddm.conf.d (default/general/zz-wayland.conf).
// Выводы apt-cache/dnf/zypper — синтетика (этих менеджеров на машине нет),
// помечена в поле `note` каждого кейса.
//
// ⛔ Тесты только читают фикстуры: ни pacman, ни pkexec, ни запись в систему.
// ══════════════════════════════════════════════════════════════════════════

use rexon_core::system_plan::{
    clean_pkg_version, filter_plasma_forks, fork_stock_candidates,
    fork_stock_target, parse_repo_version, plasma_fork_recovery_command,
    plasma_fork_reset_plan, plasma_fork_split,
    plasma_fork_unresolved_message, pkg_installed_from_rc,
    plymouth_installed_themes, plymouth_package_default,
    plymouth_reset_command, plymouth_reset_decision, plymouth_reset_outcome,
    plymouth_reset_plan, plymouth_theme_candidates, plymouth_theme_file,
    plymouth_theme_file_in, plymouth_theme_from_link, plymouth_theme_from_output,
    repo_version_query, wait_for_pkg_lock, LockWaitIo,
    log_tail_line, tail_log, LogTailKind, AUR_PROGRESS_MARKERS,
    sddm_conf_entries, sddm_conf_read_order, sddm_current_theme_from,
    sddm_distro_default_theme, sddm_files_to_remove, sddm_kde_settings_path,
    sddm_reset_command, sddm_reset_outcome, sddm_reset_plan,
    sddm_reset_success_message,
    target_plymouth_theme, installed_meta_query, installed_packages_query,
    installed_version_query, lock_wait_step,
    package_file_paths, package_files_output, package_files_query,
    parse_installed_packages, parse_installed_version, pkg_install_start_message,
    plasma_fork_preview, plasma_fork_reset_outcome, repo_target_exists,
    aur_timeout_message, TargetExistsMemo,
    PlasmaForkReset, PlymouthReset, SddmReset,
    PLYMOUTH_THEMES_DIR, SDDM_CONF_D,
};
use rexon_core::aeroshell::{rebuild_line, rebuild_outcome, rebuild_plan};

fn recovery_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_recovery_cases.json")).unwrap()
}

/// Пары (имя, содержимое) из JSON-массива `[[name, content], …]`.
fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let a = e.as_array().unwrap();
            (
                a[0].as_str().unwrap().to_string(),
                a[1].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn opt_str(v: &Value) -> Option<&str> {
    v.as_str()
}

// ── версия пакета в репозиториях ──────────────────────────────────────────

#[test]
fn clean_pkg_version_matches_python() {
    for c in recovery_fx()["clean_pkg_version"].as_array().unwrap() {
        let raw = c["raw"].as_str().unwrap();
        assert_eq!(
            clean_pkg_version(raw),
            c["result"].as_str().unwrap(),
            "clean_pkg_version({raw:?})"
        );
    }
}

#[test]
fn repo_version_query_and_parse_match_python() {
    for c in recovery_fx()["repo_version"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let family = fam(&c["family"]);
        let package = c["package"].as_str().unwrap();
        let which = c["which"].as_object().unwrap();

        let query = repo_version_query(family, package);
        // Записанные Python-вызовы safe_run: пусто = команда не запускалась.
        let calls = c["calls"].as_array().unwrap();

        let Some(q) = query else {
            assert!(calls.is_empty(), "{note}: Python команду запускал, Rust — нет");
            assert!(c["result"].is_null(), "{note}: без запроса результат должен быть None");
            continue;
        };

        // Бинаря нет → исполнитель не запускает команду (и Python не запускал).
        if !which.get(&q.binary).and_then(Value::as_bool).unwrap_or(false) {
            assert!(calls.is_empty(), "{note}: команда без бинаря");
            assert!(c["result"].is_null(), "{note}: без бинаря результат None");
            continue;
        }

        assert_eq!(calls.len(), 1, "{note}: ровно один запуск");
        assert_eq!(json!(q.cmd.argv), calls[0]["argv"], "{note}: argv");
        assert_eq!(json!(q.cmd.timeout_secs), calls[0]["timeout"], "{note}: timeout");
        assert_eq!(json!(q.cmd.lang_c), calls[0]["lang_c"], "{note}: LANG=C");

        // Гейты, которые в Python стоят до разбора.
        let rc = c["rc"].as_i64().unwrap();
        let stdout = c["stdout"].as_str().unwrap();
        let got = if stdout.is_empty() || (q.check_returncode && rc != 0) {
            None
        } else {
            parse_repo_version(family, stdout)
        };
        assert_eq!(json!(got), c["result"], "{note}: версия");
    }
}

// ── форки Plasma ──────────────────────────────────────────────────────────

/// Колбэк «stdout pacman -Qi по пакету» из фикстуры (нет ключа = rc != 0).
fn qi_from(fx: &Value) -> impl Fn(&str) -> Option<String> + '_ {
    let map = fx["plasma_forks"]["qi_fixtures"].as_object().unwrap();
    move |pkg: &str| map.get(pkg).and_then(Value::as_str).map(String::from)
}

/// Колбэк «есть ли цель в репозиториях» из фикстуры `repo_targets` (снята
/// `pacman -Si` на живой машине). Нет ключа — цели в репозиториях нет
/// (`Some(false)`), как и отвечает pacman: сам пакет он знает поимённо.
fn exists_from(fx: &Value) -> impl Fn(&str) -> Option<bool> + '_ {
    let map = fx["plasma_forks"]["repo_targets"].as_object().unwrap();
    move |pkg: &str| Some(map.get(pkg).and_then(Value::as_bool).unwrap_or(false))
}

#[test]
fn filter_plasma_forks_matches_python() {
    let fx = recovery_fx();
    for c in fx["plasma_forks"]["filter"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(filter_plasma_forks(&str_vec(&c["input"]))),
            c["result"],
            "filter_plasma_forks: {note}"
        );
    }
}

#[test]
fn parse_qi_field_matches_python_on_live_output() {
    let fx = recovery_fx();
    let map = fx["plasma_forks"]["qi_fixtures"].as_object().unwrap();
    for c in fx["plasma_forks"]["parse_qi_field"].as_array().unwrap() {
        let pkg = c["package"].as_str().unwrap();
        let field = c["field"].as_str().unwrap();
        let stdout = map[pkg].as_str().unwrap();
        assert_eq!(
            json!(rexon_core::system::parse_qi_field(stdout, field)),
            c["result"],
            "parse_qi_field({pkg}, {field})"
        );
    }
}

#[test]
fn fork_stock_candidates_prefer_provides_over_stale_replaces() {
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    for c in fx["plasma_forks"]["stock_candidates"].as_array().unwrap() {
        let fork = c["fork"].as_str().unwrap();
        assert_eq!(
            json!(fork_stock_candidates(fork, &qi)),
            c["result"],
            "fork_stock_candidates({fork})"
        );
    }
}

#[test]
fn fork_stock_target_checks_repo_existence() {
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    for c in fx["plasma_forks"]["stock_target"].as_array().unwrap() {
        let fork = c["fork"].as_str().unwrap();
        assert_eq!(
            json!(fork_stock_target(fork, &qi, &exists)),
            c["result"],
            "fork_stock_target({fork})"
        );
    }
}

/// Живой `pacman -Qi` форка владельца: `replaces` тянет `plasma-framework`
/// со времён Plasma 5, `provides` уже `libplasma`. До проверки существования
/// откат собирал `pacman -S plasma-framework` и падал «не найдена цель».
#[test]
fn stale_replaces_never_reaches_the_command() {
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let live = fx["plasma_forks"]["qi_fixtures"]["aeroshell-libplasma-git"]
        .as_str()
        .unwrap();
    assert!(
        live.contains("Replaces        : plasma-framework"),
        "фикстура должна остаться живым выводом с протухшим replaces"
    );
    // Без проверки имя-покойник стоит в кандидатах…
    assert_eq!(
        fork_stock_candidates("aeroshell-libplasma-git", &qi),
        vec!["libplasma".to_string(), "plasma-framework".to_string()]
    );
    // …но в команду не попадает ни при какой проверке репозиториев.
    let forks = vec!["aeroshell-libplasma-git".to_string()];
    let live_repos = exists_from(&fx);
    assert_eq!(
        plasma_fork_recovery_command(&forks, &qi, &live_repos).unwrap(),
        "sudo pacman -S libplasma"
    );
    // Даже когда спросить репозитории нечем (pacman недоступен) — берётся
    // первый кандидат, а первый теперь provides, а не replaces.
    let cannot_ask = |_: &str| None;
    assert_eq!(
        plasma_fork_recovery_command(&forks, &qi, &cannot_ask).unwrap(),
        "sudo pacman -S libplasma"
    );
}

/// Форк, у которого живого имени не осталось: откат обязан отказать ЦЕЛИКОМ,
/// не сняв ни одного пакета, — иначе система остаётся без стока.
#[test]
fn unresolved_fork_blocks_the_whole_reset() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    let forks = vec![
        "aeroshell-ghost-git".to_string(),
        "aeroshell-smod-git".to_string(),
    ];
    match plasma_fork_reset_plan(&forks, &qi, &exists) {
        PlasmaForkReset::Blocked { unresolved, message, .. } => {
            assert_eq!(unresolved.len(), 1);
            assert_eq!(unresolved[0].fork, "aeroshell-ghost-git");
            assert_eq!(unresolved[0].candidates, vec!["plasma-framework".to_string()]);
            assert!(message.contains("aeroshell-ghost-git"), "{message}");
            assert!(message.contains("plasma-framework"), "{message}");
            assert!(message.contains("Ничего не удалено"), "{message}");
            assert_eq!(message, plasma_fork_unresolved_message(&unresolved));
        }
        other => panic!("ожидался Blocked, получено {other:?}"),
    }
}

#[test]
fn plasma_fork_split_and_recovery_match_python() {
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    for c in fx["plasma_forks"]["split"].as_array().unwrap() {
        let forks = str_vec(&c["forks"]);
        let split = plasma_fork_split(&forks, &qi, &exists);
        assert_eq!(json!(split.removable), c["removable"], "split removable {forks:?}");
        assert_eq!(json!(split.stock), c["stock"], "split stock {forks:?}");
        assert_eq!(
            json!(split.unresolved), c["unresolved"],
            "split unresolved {forks:?}"
        );
    }
    for c in fx["plasma_forks"]["recovery"].as_array().unwrap() {
        let forks = str_vec(&c["forks"]);
        assert_eq!(
            json!(plasma_fork_recovery_command(&forks, &qi, &exists)),
            c["result"],
            "recovery_command {forks:?}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plasma_fork_reset_plan_matches_python_trace() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    for c in fx["plasma_forks"]["reset"].as_array().unwrap() {
        let forks = str_vec(&c["forks"]);
        let calls = c["pkexec_calls"].as_array().unwrap();
        match plasma_fork_reset_plan(&forks, &qi, &exists) {
            PlasmaForkReset::NoForks { ok, message } => {
                assert!(ok, "{forks:?}: «форков нет» — это не отказ");
                assert_eq!(json!(message), c["msg"], "{forks:?}");
                assert!(calls.is_empty(), "{forks:?}: pkexec не должен вызываться");
            }
            PlasmaForkReset::NoActions { ok, message } => {
                assert!(ok, "{forks:?}: «нет действий» — это не отказ");
                assert_eq!(json!(message), c["msg"], "{forks:?}");
                assert!(calls.is_empty(), "{forks:?}: pkexec не должен вызываться");
            }
            PlasmaForkReset::Blocked { ok, unresolved, message } => {
                assert!(!ok, "{forks:?}: Blocked — это отказ");
                assert_eq!(c["kind"], "Blocked", "{forks:?}: ожидался Blocked");
                assert!(calls.is_empty(), "{forks:?}: pkexec НЕ запускается");
                assert_eq!(json!(unresolved), c["unresolved"], "{forks:?}");
                assert!(!message.is_empty(), "{forks:?}: отказ без объяснения");
            }
            PlasmaForkReset::Run { cmd, shell, removable, stock } => {
                assert_eq!(calls.len(), 1, "{forks:?}: один pkexec");
                assert_eq!(json!(cmd.argv), calls[0]["argv"], "{forks:?}: argv");
                assert_eq!(
                    json!(cmd.timeout_secs), calls[0]["timeout"],
                    "{forks:?}: timeout"
                );
                // shell — то же тело, что в argv[2] записанного вызова.
                assert_eq!(
                    format!("set -e; {shell}"),
                    calls[0]["argv"][3].as_str().unwrap(),
                    "{forks:?}: тело скрипта"
                );
                let split = plasma_fork_split(&forks, &qi, &exists);
                assert_eq!(removable, split.removable, "{forks:?}: удаляемые в плане");
                assert_eq!(stock, split.stock, "{forks:?}: сток в плане");
            }
        }
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plasma_fork_preview_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    for c in fx["plasma_forks"]["preview"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let answer = plasma_fork_preview(&str_vec(&c["forks"]), &qi, &exists);
        assert_eq!(json!(answer.ok), c["ok"], "preview: {note}");
        assert_eq!(json!(answer.message), c["message"], "preview: {note}");
    }
}

/// Предпросмотр проходит по форкам ДВАЖДЫ (план + человекочитаемая команда),
/// а `pacman -Si` спрашивается по одному разу на имя — за это отвечает
/// память пробы. Без неё каждый повтор стоил бы отдельного запуска.
#[test]
fn plasma_fork_preview_asks_repositories_once_per_name() {
    let fx = recovery_fx();
    let qi = qi_from(&fx);
    let exists = exists_from(&fx);
    let calls = std::cell::Cell::new(0usize);
    let counted = |pkg: &str| {
        calls.set(calls.get() + 1);
        exists(pkg)
    };
    let forks = vec![
        "aeroshell-libplasma-git".to_string(),
        "aeroshell-libplasma".to_string(),
    ];
    let answer = plasma_fork_preview(&forks, &qi, &counted);
    assert!(answer.ok, "откат по этой паре возможен");
    // Оба форка объявляют один и тот же сток libplasma — цель ОДНА.
    let with_memo = calls.get();
    assert_eq!(with_memo, 1, "спрошено больше имён, чем разных целей");

    // Без памяти те же два прохода спросили бы про libplasma каждый раз.
    let raw = std::cell::Cell::new(0usize);
    let uncounted = |pkg: &str| {
        raw.set(raw.get() + 1);
        exists(pkg)
    };
    let _ = plasma_fork_reset_plan(&forks, &qi, &uncounted);
    let _ = plasma_fork_recovery_command(&forks, &qi, &uncounted);
    assert!(
        raw.get() > with_memo,
        "память пробы ничего не экономит: {} против {with_memo}",
        raw.get()
    );
}

#[test]
fn target_exists_memo_asks_each_name_once() {
    let asked = std::cell::Cell::new(0usize);
    let probe = |_pkg: &str| {
        asked.set(asked.get() + 1);
        None::<bool>
    };
    let mut memo = TargetExistsMemo::new();
    for _ in 0..3 {
        assert_eq!(memo.answer("libplasma", &probe), None);
    }
    // Даже «спросить не удалось» запоминается: ответ не изменится, а каждый
    // повтор стоил бы запуска pacman.
    assert_eq!(asked.get(), 1, "None не запомнился");
    assert_eq!(memo.asked(), 1);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plasma_fork_reset_outcome_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in recovery_fx()["plasma_forks"]["outcome"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let planned = opt_str(&c["planned"]).map(String::from);
        let remaining = opt_str(&c["remaining"]).map(String::from);
        let answer = plasma_fork_reset_outcome(
            c["shell"].as_str().unwrap(),
            c["returncode"].as_i64().map(|v| v as i32),
            c["stdout"].as_str().unwrap(),
            c["stderr"].as_str().unwrap(),
            &|| planned.clone(),
            &|| remaining.clone(),
        );
        assert_eq!(json!(answer.ok), c["ok"], "outcome: {note}");
        assert_eq!(json!(answer.message), c["message"], "outcome: {note}");
    }
}

/// ⭐ Починка при переносе: подсказка «выполни вручную» больше не может
/// содержать строку «None». Прежний Python подставлял `str(None)`.
#[test]
fn denied_hint_never_says_none() {
    for c in recovery_fx()["plasma_forks"]["outcome"].as_array().unwrap() {
        let Some(was) = c["was_python"].as_str() else { continue };
        assert!(was.contains("None"), "кейс починки не про «None»: {c}");
        let message = c["message"].as_str().unwrap();
        assert!(!message.contains("None"), "«None» доехало до человека: {c}");
        assert!(
            message.contains(c["shell"].as_str().unwrap()),
            "вместо команды отката должно стоять тело транзакции: {c}"
        );
    }
}

/// ⭐ Сторож языка отказа пароля в операциях восстановления (форки Plasma,
/// Plymouth, SDDM). Прежний текст «Авторизация pkexec отклонена.» говорил
/// покупателю служебные слова и приписывал отказ, когда человек просто
/// закрыл окно пароля. Первая строка обязана совпадать с отказами установки
/// и удаления, команда для терминала — доезжать как есть.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn denied_message_speaks_plain_russian() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let cmd = "sudo rm -f /etc/sddm.conf.d/zz-rexon-theme.conf";
    let msg = rexon_core::system_plan::pkexec_denied_message(cmd);
    let words = msg.strip_suffix(cmd).expect("команда для терминала — в конце, как есть");
    for jargon in ["pkexec", "polkit", "Авторизац", "отклонен", "хелпер"] {
        assert!(!words.contains(jargon), "в тексте для покупателя «{jargon}»: {msg}");
    }
    assert!(msg.starts_with("Пароль администратора не подтверждён.\n"), "{msg}");
}

/// ⭐ Тот же сторож для случая, когда запросить пароль нечем. Прежний текст
/// «pkexec не найден» говорил служебное слово и не говорил, что делать.
/// Имена пакетов допустимы только в строке-инструкции: это то, что надо
/// поставить, а не объяснение.
#[test]
fn missing_password_prompt_message_speaks_plain_russian() {
    let _lang = i18n_pin::ru();
    let msg = rexon_core::system_plan::pkexec_missing_message();
    let (explanation, instruction) = msg.split_once('\n').expect("объяснение и что делать — две строки");
    for jargon in ["pkexec", "polkit", "Авторизац", "хелпер", "не найден"] {
        assert!(!explanation.contains(jargon), "в объяснении для покупателя «{jargon}»: {msg}");
    }
    assert!(explanation.contains("пароль администратора"), "{msg}");
    assert!(instruction.starts_with("Установите пакет "), "что делать — на «вы»: {msg}");
}

/// ⭐ Тот же сторож для установки системных тем (экран входа, заставка
/// загрузки) во «Оформлении». Прежде там говорили «pkexec не найден»,
/// «Системный helper Rexon не установлен (<путь>)» и «(pkexec/polkit)».
/// Отказ «нечем запросить пароль» обязан быть тем же текстом, что в
/// восстановлении, — не второй копией фразы.
#[test]
fn appearance_root_install_texts_speak_plain_russian() {
    use rexon_core::appearance_installer::{self as ai, RootEnv};
    let sddm = rexon_core::categories::category_by_id(std::path::Path::new("/sb/home"), 101).unwrap();
    let env = RootEnv {
        is_plasmalogin: false,
        has_pkexec: false,
        system_helper_path: "/usr/local/lib/rexon/system-helper",
        system_helper_exists: true,
        is_steamos_or_immutable: false,
        is_steamos: false,
        family_is_debian: false,
    };
    let no_prompt = ai::root_preflight(&env, &sddm).unwrap().message;
    // Та же фраза, что в восстановлении: одна функция каталога перевода.
    assert_eq!(no_prompt, rexon_core::system_plan::pkexec_missing_message());

    let env = RootEnv { has_pkexec: true, system_helper_exists: false, ..env };
    let no_system_part = ai::root_preflight(&env, &sddm).unwrap().message;
    assert!(!no_system_part.contains(env.system_helper_path), "путь к файлу в тексте: {no_system_part}");

    let cancelled = ai::root_cancelled_message();
    for msg in [no_system_part.as_str(), cancelled.as_str()] {
        for jargon in ["pkexec", "polkit", "helper", "хелпер", "Авторизац"] {
            assert!(!msg.contains(jargon), "в тексте для покупателя «{jargon}»: {msg}");
        }
    }
    // Сами слова отказа (по-русски и по-английски) держат сверки установщика
    // тем — `appearance_installer_parity`: там язык закреплён.
}

// ── движок пакетов: «перечислить всё установленное» ───────────────────────

#[test]
fn parse_installed_packages_matches_python() {
    let fx = recovery_fx();
    for c in fx["plasma_forks"]["installed_packages"]["parse"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = parse_installed_packages(
            c["ok"].as_bool().unwrap(),
            c["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got.len()), c["count"], "installed_packages: {note}");
    }
}

/// Живой `pacman -Qq` этой машины проходит через движок и фильтр так же, как
/// проходил через зашитый `r.stdout.split()`.
#[test]
fn installed_packages_feed_the_fork_filter() {
    let fx = recovery_fx();
    let live = fx["plasma_forks"]["installed_packages"]["parse"][0]["stdout"]
        .as_str()
        .unwrap();
    let names = parse_installed_packages(true, live);
    assert!(names.len() > 100, "живой список подозрительно короток");
    assert_eq!(
        json!(filter_plasma_forks(&names)),
        fx["plasma_forks"]["installed_packages"]["live_forks"],
        "живой список даёт не те форки, что записаны в голдене"
    );
}

#[test]
fn installed_packages_query_only_where_format_is_known() {
    let arch = installed_packages_query(PkgFamily::Arch).unwrap();
    assert_eq!(arch.binary, "pacman");
    assert_eq!(arch.cmd.argv, vec!["pacman", "-Qq"]);
    assert!(arch.cmd.lang_c, "разбор без LANG=C ломается на русской локали");
    assert!(installed_packages_query(PkgFamily::Debian).is_some());
    assert!(installed_packages_query(PkgFamily::Fedora).is_some());
    // Формат ответа руками не проверен — честное «не знаю» вместо выдумки.
    for f in [PkgFamily::Gentoo, PkgFamily::Void, PkgFamily::Nix,
              PkgFamily::Alpine, PkgFamily::Solus, PkgFamily::Clear,
              PkgFamily::Unknown] {
        assert!(installed_packages_query(f).is_none(), "{f:?}");
    }
}

#[test]
fn installed_meta_query_is_arch_only_and_validates_name() {
    let q = installed_meta_query(PkgFamily::Arch, "aeroshell-libplasma-git").unwrap();
    assert_eq!(q.cmd.argv, vec!["pacman", "-Qi", "aeroshell-libplasma-git"]);
    assert!(q.cmd.lang_c);
    // Поля `pacman -Qi` разбирает `parse_qi_field`; у dpkg/rpm они зовутся
    // иначе, и молча отдавать их тому же разбору нельзя.
    assert!(installed_meta_query(PkgFamily::Debian, "libplasma").is_none());
    assert!(installed_meta_query(PkgFamily::Arch, "плохое имя").is_none());
    assert!(installed_meta_query(PkgFamily::Arch, "").is_none());
}

#[test]
fn repo_target_exists_matches_live_pacman() {
    for c in recovery_fx()["plasma_forks"]["repo_target_exists"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let family = fam(&c["family"]);
        let got = repo_target_exists(
            family,
            c["returncode"].as_i64().map(|v| v as i32),
            c["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got), c["result"], "repo_target_exists: {note}");
    }
}

// ── склонение «пакет(ов)» ─────────────────────────────────────────────────

/// Покупателю показывали скобки вместо русского слова. Держится отдельным
/// тестом, чтобы не выпасть при пересъёмке голденов.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn package_counts_are_declined_not_bracketed() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for (n, install, timeout) in [
        (1usize, "📦 Установка 1 пакета…", "(1 пакет)"),
        (2, "📦 Установка 2 пакетов…", "(2 пакета)"),
        (5, "📦 Установка 5 пакетов…", "(5 пакетов)"),
        (21, "📦 Установка 21 пакета…", "(21 пакет)"),
    ] {
        assert_eq!(pkg_install_start_message(n, false), install);
        assert!(
            aur_timeout_message(n).contains(timeout),
            "таймаут AUR на {n}: {}",
            aur_timeout_message(n)
        );
        assert!(!pkg_install_start_message(n, true).contains("пакет(ов)"));
    }
    assert_eq!(
        pkg_install_start_message(1, true),
        "📦 SteamOS: установка 1 пакета…"
    );
}

// ── Plymouth ──────────────────────────────────────────────────────────────

#[test]
fn plymouth_theme_from_link_matches_python() {
    for c in recovery_fx()["plymouth"]["theme_from_link"].as_array().unwrap() {
        let target = opt_str(&c["target"]);
        assert_eq!(
            json!(plymouth_theme_from_link(target)),
            c["result"],
            "theme_from_link({target:?})"
        );
    }
}

#[test]
fn plymouth_package_default_matches_python() {
    for c in recovery_fx()["plymouth"]["package_default"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(plymouth_package_default(opt_str(&c["text"]))),
            c["result"],
            "package_default: {note}"
        );
    }
}

#[test]
fn target_plymouth_theme_matches_python_including_laziness() {
    for c in recovery_fx()["plymouth"]["target"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let themes = str_vec(&c["themes"]);
        let installed = str_vec(&c["installed_pkgs"]);
        let defaults = c["defaults"].as_str().map(String::from);

        // Записываем обращения к pacman -Qq, чтобы сверить ленивость 1-в-1.
        let queried = std::cell::RefCell::new(Vec::<String>::new());
        let pkg_installed = |p: &str| {
            queried.borrow_mut().push(p.to_string());
            installed.iter().any(|x| x == p)
        };
        let default_calls = std::cell::Cell::new(0u32);
        let package_default = || {
            default_calls.set(default_calls.get() + 1);
            plymouth_package_default(defaults.as_deref())
        };

        let got = target_plymouth_theme(&themes, &pkg_installed, &package_default);
        assert_eq!(json!(got), c["result"], "target_plymouth_theme: {note}");
        assert_eq!(
            json!(queried.into_inner()), c["pkg_queries"],
            "target_plymouth_theme: {note} — набор и порядок проб pacman"
        );
        // plymouthd.defaults читается максимум один раз и только после
        // отпавшего брендинга (в Python — та же ленивость).
        assert!(default_calls.get() <= 1, "{note}: лишнее чтение plymouthd.defaults");
    }
}

#[test]
fn plymouth_reset_command_matches_python() {
    for c in recovery_fx()["plymouth"]["reset_command"].as_array().unwrap() {
        let target = c["target"].as_str().unwrap();
        let has_tool = c["has_tool"].as_bool().unwrap();
        assert_eq!(
            json!(plymouth_reset_command(target, has_tool)),
            c["result"],
            "plymouth_reset_command({target:?}, {has_tool})"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plymouth_reset_plan_matches_python_trace() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in recovery_fx()["plymouth"]["reset_plan"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let target = opt_str(&c["target"]);
        let current = opt_str(&c["current"]);
        let isfile: Vec<String> = str_vec(&c["isfile"]);
        let exists = |p: &str| isfile.iter().any(|x| x == p);
        let calls = c["pkexec_calls"].as_array().unwrap();

        let plan = plymouth_reset_plan(
            target,
            current,
            c["tool"].as_bool().unwrap(),
            c["alt"].as_bool().unwrap(),
            &exists,
        );
        let msg = c["msg"].as_str().unwrap();

        // Тексты ответов держит ядро: у исходов без pkexec решение приходит
        // готовым, у Run — после нулевого кода возврата.
        let decision = plymouth_reset_decision(
            target,
            current,
            c["tool"].as_bool().unwrap(),
            c["alt"].as_bool().unwrap(),
            &exists,
        );
        assert_eq!(decision.plan, plan, "{note}: решение и план разошлись");
        match &decision.plan {
            PlymouthReset::Run { shell, target, .. } => {
                assert_eq!(decision.ok, None, "{note}: Run отвечает после запуска");
                assert_eq!(decision.message, None, "{note}");
                let out = plymouth_reset_outcome(target, shell, Some(0), "", "");
                assert_eq!(json!(out.ok), c["ok"], "{note}: ok после pkexec");
                assert_eq!(out.message, msg, "{note}: текст после pkexec");
            }
            _ => {
                assert_eq!(json!(decision.ok), c["ok"], "{note}: ok без pkexec");
                assert_eq!(
                    decision.message.as_deref(), Some(msg),
                    "{note}: текст без pkexec"
                );
            }
        }
        match plan {
            PlymouthReset::NoTarget => {
                assert_eq!(msg, "Не удалось определить родную тему Plymouth дистрибутива", "{note}");
                assert!(calls.is_empty(), "{note}");
            }
            PlymouthReset::AlreadyDefault { target } => {
                assert_eq!(msg, format!("Plymouth уже на стандартной теме «{target}»"), "{note}");
                assert!(calls.is_empty(), "{note}");
            }
            PlymouthReset::MissingThemeFile { target, path } => {
                assert_eq!(msg, format!("В теме «{target}» нет файла {target}.plymouth"), "{note}");
                assert!(path.ends_with(".plymouth"), "{note}: путь alternatives");
                assert!(calls.is_empty(), "{note}");
            }
            PlymouthReset::NoTool => {
                assert_eq!(
                    msg,
                    "Нечем переключить заставку загрузки: не найдены ни \
                     plymouth-set-default-theme, ни update-alternatives",
                    "{note}"
                );
                assert!(calls.is_empty(), "{note}");
            }
            PlymouthReset::Run { cmd, shell, target } => {
                assert_eq!(calls.len(), 1, "{note}: один pkexec");
                assert_eq!(json!(cmd.argv), calls[0]["argv"], "{note}: argv");
                assert_eq!(json!(cmd.timeout_secs), calls[0]["timeout"], "{note}: timeout");
                assert_eq!(
                    format!("set -e; {shell}"),
                    calls[0]["argv"][3].as_str().unwrap(),
                    "{note}: тело скрипта"
                );
                assert!(!target.is_empty(), "{note}: цель в плане");
            }
        }
    }
}

/// Что считать установленной темой: отбор подкаталогов и проба
/// `<тема>.plymouth`. Каталог здесь не читается — вход подан списком.
#[test]
fn plymouth_theme_list_matches_golden() {
    for c in recovery_fx()["plymouth"]["theme_list"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        if c["entries"].is_null() {
            // Каталога тем нет вовсе: перечислять нечего, ядро не зовётся —
            // Python отдаёт пустой список сразу после OSError.
            assert!(
                c["result"].as_array().unwrap().is_empty(),
                "{note}: у отсутствующего каталога тем быть не может"
            );
            continue;
        }
        let entries: Vec<(String, bool)> = c["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let a = e.as_array().unwrap();
                (a[0].as_str().unwrap().to_string(), a[1].as_bool().unwrap())
            })
            .collect();
        let with_file = str_vec(&c["themes_with_file"]);

        let candidates = plymouth_theme_candidates(&entries);
        // Файл темы пробуется ТОЛЬКО у отобранных записей.
        let probes: Vec<(String, bool)> = candidates
            .iter()
            .map(|t| (t.clone(), with_file.iter().any(|x| x == t)))
            .collect();
        assert_eq!(
            json!(plymouth_installed_themes(&probes)),
            c["result"],
            "список тем: {note}"
        );

        // Порядок обхода каталога произволен, ответ от него не зависит.
        let mut shuffled = entries.clone();
        shuffled.reverse();
        let probes_rev: Vec<(String, bool)> = plymouth_theme_candidates(&shuffled)
            .iter()
            .map(|t| (t.clone(), with_file.iter().any(|x| x == t)))
            .collect();
        assert_eq!(
            json!(plymouth_installed_themes(&probes_rev)),
            c["result"],
            "порядок каталога не должен влиять: {note}"
        );
    }
}

/// ⚠️ Починка при порте, держится отдельно от голдена: темой подкаталог
/// делает файл `<тема>.plymouth`. Оригинал отдавал ВСЕ подкаталоги, и пустой
/// каталог от удалённого пакета мог стать «родным дефолтом дистрибутива».
#[test]
fn plymouth_theme_without_its_file_is_not_a_theme() {
    let entries = vec![("bgrt".to_string(), true), ("spinner".to_string(), true)];
    let candidates = plymouth_theme_candidates(&entries);
    assert_eq!(candidates, vec!["bgrt".to_string(), "spinner".to_string()]);
    let probes = vec![("bgrt".to_string(), false), ("spinner".to_string(), true)];
    assert_eq!(
        plymouth_installed_themes(&probes),
        vec!["spinner".to_string()],
        "каталог без своего .plymouth темой не считается"
    );
    // На живой машине ответ прежний: у всех 13 тем файл на месте.
    let live = str_vec(&recovery_fx()["plymouth"]["live_themes"]);
    let all: Vec<(String, bool)> = live.iter().map(|t| (t.clone(), true)).collect();
    assert_eq!(plymouth_installed_themes(&all), live, "живой список не изменился");
    assert_eq!(
        plymouth_theme_file("bgrt"),
        "/usr/share/plymouth/themes/bgrt/bgrt.plymouth",
        "путь описания темы"
    );
    // Каталог тем задаётся аргументом: проба файла идёт в ТОТ ЖЕ каталог,
    // который перечислили, а не в зашитый /usr/share.
    assert_eq!(
        plymouth_theme_file_in(PLYMOUTH_THEMES_DIR, "bgrt"),
        plymouth_theme_file("bgrt"),
        "боевой путь не изменился"
    );
    assert_eq!(
        plymouth_theme_file_in("/var/tmp/plymouth-preview/", "bgrt"),
        "/var/tmp/plymouth-preview/bgrt/bgrt.plymouth",
        "подставленный каталог не должен уводить в /usr/share"
    );
}

/// Разбор вывода `plymouth-set-default-theme` (без аргументов он печатает имя).
#[test]
fn plymouth_theme_from_output_matches_golden() {
    for c in recovery_fx()["plymouth"]["detect_output"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(plymouth_theme_from_output(
                c["returncode"].as_i64().map(|v| v as i32),
                c["stdout"].as_str().unwrap()
            )),
            c["result"],
            "detect: {note}"
        );
    }
}

/// Установлен ли пакет — по коду возврата команды проверки.
#[test]
fn pkg_installed_from_rc_matches_golden() {
    for c in recovery_fx()["plymouth"]["pkg_installed"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(pkg_installed_from_rc(c["returncode"].as_i64().map(|v| v as i32))),
            c["result"],
            "pkg_installed: {note}"
        );
    }
}

/// Итог pkexec для Plymouth: код возврата и оба потока → (ok, сообщение).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plymouth_reset_outcome_matches_golden() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in recovery_fx()["plymouth"]["reset_outcome"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = plymouth_reset_outcome(
            c["target"].as_str().unwrap(),
            c["shell"].as_str().unwrap(),
            c["returncode"].as_i64().map(|v| v as i32),
            c["stdout"].as_str().unwrap(),
            c["stderr"].as_str().unwrap(),
        );
        assert_eq!(json!(got.ok), c["ok"], "{note}: ok");
        assert_eq!(got.message, c["message"].as_str().unwrap(), "{note}: текст");
    }
}

// ── SDDM ──────────────────────────────────────────────────────────────────

#[test]
fn sddm_files_to_remove_matches_python() {
    for c in recovery_fx()["sddm"]["files_to_remove"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(sddm_files_to_remove(&pairs(&c["entries"]))),
            c["result"],
            "sddm_files_to_remove: {note}"
        );
    }
}

#[test]
fn sddm_current_theme_matches_python() {
    for c in recovery_fx()["sddm"]["current_theme"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(sddm_current_theme_from(&pairs(&c["entries"]))),
            c["result"],
            "sddm_current_theme: {note}"
        );
    }
}

#[test]
fn sddm_distro_default_matches_python() {
    for c in recovery_fx()["sddm"]["distro_default"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(sddm_distro_default_theme(&pairs(&c["distro"]), opt_str(&c["kde"]))),
            c["result"],
            "sddm_distro_default: {note}"
        );
    }
}

/// Живая машина целиком: план сброса на реальных /etc/sddm.conf.d и
/// /usr/lib/sddm/sddm.conf.d должен быть «нечего удалять, дефолт breeze».
#[test]
fn sddm_reset_plan_on_live_machine() {
    let fx = recovery_fx();
    let etc = pairs(&fx["sddm"]["live_etc"]);
    let usr = pairs(&fx["sddm"]["live_usr"]);
    let kde = opt_str(&fx["sddm"]["live_kde"]);
    let plan = sddm_reset_plan(&etc, &usr, kde);
    assert!(
        plan.to_remove.is_empty(),
        "на живой машине Rexon-оверрайдов нет — удалять нечего, получили {:?}",
        plan.to_remove
    );
    // Тот же ответ, что у Python-эталона на этих же данных.
    let want = fx["sddm"]["distro_default"][0]["result"].as_str();
    assert_eq!(plan.default_theme.as_deref(), want, "дистро-дефолт живой машины");
    assert!(matches!(sddm_reset_command(&plan, "/etc/sddm.conf.d"), SddmReset::AlreadyStock));
}

#[test]
fn sddm_reset_command_matches_python_trace() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    for c in recovery_fx()["sddm"]["reset_command"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let plan = rexon_core::system_plan::SddmResetPlan {
            to_remove: str_vec(&c["to_remove"]),
            default_theme: c["default_theme"].as_str().map(String::from),
        };
        let conf_d = c["conf_d"].as_str().unwrap();
        let calls = c["pkexec_calls"].as_array().unwrap();
        match sddm_reset_command(&plan, conf_d) {
            SddmReset::AlreadyStock => {
                assert_eq!(
                    c["msg"], "Экран входа (SDDM) уже в исходных настройках",
                    "{note}"
                );
                assert!(calls.is_empty(), "{note}: pkexec не должен вызываться");
            }
            SddmReset::Run { cmd, shell, removed, default_theme } => {
                assert_eq!(calls.len(), 1, "{note}: один pkexec");
                assert_eq!(json!(cmd.argv), calls[0]["argv"], "{note}: argv");
                assert_eq!(json!(cmd.timeout_secs), calls[0]["timeout"], "{note}: timeout");
                assert_eq!(
                    format!("set -e; {shell}"),
                    calls[0]["argv"][3].as_str().unwrap(),
                    "{note}: тело скрипта"
                );
                // Текст успеха — байт-в-байт с Python.
                assert_eq!(
                    sddm_reset_success_message(&removed, default_theme.as_deref()),
                    c["msg"].as_str().unwrap(),
                    "{note}: сообщение успеха"
                );
            }
        }
    }
}

/// Чистая часть `_sddm_conf_entries`: отбор `.conf`, порядок мёрджа и пустой
/// текст нечитаемому файлу. Каталог здесь не читается — вход подан списком.
#[test]
fn sddm_conf_entries_matches_golden() {
    for c in recovery_fx()["sddm"]["conf_entries"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        if c["names"].is_null() {
            // Каталога нет вовсе: перечислять нечего, ядро не зовётся —
            // Python отдаёт пустой список сразу после OSError.
            assert!(
                c["result"].as_array().unwrap().is_empty(),
                "{note}: у отсутствующего каталога записей быть не может"
            );
            continue;
        }
        let names = str_vec(&c["names"]);
        let order = sddm_conf_read_order(&names);
        assert_eq!(json!(order), c["read_order"], "порядок чтения: {note}");

        let contents = c["contents"].as_object().unwrap();
        let read = |n: &String| (n.clone(), contents[n].as_str().map(String::from));
        let reads: Vec<(String, Option<String>)> = order.iter().map(read).collect();
        assert_eq!(json!(sddm_conf_entries(&reads)), c["result"], "сборка пар: {note}");

        // Порядок мёрджа задаёт ядро, а не порядок обхода каталога: те же
        // файлы, поданные как их отдал listdir, дают тот же ответ.
        let raw: Vec<(String, Option<String>)> = names
            .iter()
            .filter(|n| n.ends_with(".conf"))
            .map(read)
            .collect();
        assert_eq!(
            json!(sddm_conf_entries(&raw)),
            c["result"],
            "порядок каталога не должен влиять: {note}"
        );
    }
}

#[test]
fn sddm_kde_settings_path_matches_golden() {
    for c in recovery_fx()["sddm"]["kde_settings_path"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            sddm_kde_settings_path(c["conf_d"].as_str().unwrap()),
            c["result"].as_str().unwrap(),
            "sddm_kde_settings_path: {note}"
        );
    }
}

/// ⚠️ Починка при порте, держится отдельно от голдена: `kde_settings.conf`
/// ищется в ТОМ ЖЕ каталоге, который сканируют, а не в зашитом /etc. Оригинал
/// склеивал путь с `_SDDM_CONF_D` и на подставленном каталоге читал настоящий
/// файл живой машины.
#[test]
fn sddm_kde_settings_path_follows_conf_d() {
    assert_eq!(
        sddm_kde_settings_path(SDDM_CONF_D),
        "/etc/sddm.conf.d/kde_settings.conf",
        "боевой путь не изменился"
    );
    assert_ne!(
        sddm_kde_settings_path("/var/tmp/sddm-preview"),
        sddm_kde_settings_path(SDDM_CONF_D),
        "подставленный каталог не должен уводить в /etc"
    );
}

/// Итог pkexec: код возврата и оба потока → (ok, сообщение, удалённые).
#[test]
fn sddm_reset_outcome_matches_golden() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    for c in recovery_fx()["sddm"]["reset_outcome"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = sddm_reset_outcome(
            &str_vec(&c["removed"]),
            c["default_theme"].as_str(),
            c["shell"].as_str().unwrap(),
            c["returncode"].as_i64().map(|v| v as i32),
            c["stdout"].as_str().unwrap(),
            c["stderr"].as_str().unwrap(),
        );
        assert_eq!(json!(got.ok), c["ok"], "{note}: ok");
        assert_eq!(got.message, c["message"].as_str().unwrap(), "{note}: текст");
        assert_eq!(json!(got.removed), c["result_removed"], "{note}: удалённые");
    }
}

// ── детект экрана входа ───────────────────────────────────────────────────
//
// Живая машина (CachyOS/KDE) сидит на Plasma Login Manager: симлинк
// display-manager.service → plasmalogin.service, `is-enabled plasmalogin` =
// enabled, sddm/lightdm = disabled, gdm/cosmic-greeter = not-found. Эти
// состояния зафиксированы в голдене, поэтому тест не зовёт systemctl.

#[test]
fn login_manager_tables_match_python() {
    let fx = recovery_fx();
    let lm = &fx["login_manager"];
    assert_eq!(json!(rexon_core::system_plan::KNOWN_DM_UNITS), lm["known_units"]);
    assert_eq!(
        json!(rexon_core::system_plan::LOGIN_MANAGER_PROBE_UNITS),
        lm["probe_units"]
    );
    assert_eq!(
        json!(rexon_core::system_plan::DM_ENABLED_STATES),
        lm["enabled_states"]
    );
    assert_eq!(rexon_core::system_plan::DEFAULT_LOGIN_MANAGER, lm["default"]);
    assert_eq!(rexon_core::system_plan::DM_SYMLINK, lm["symlink"]);
}

#[test]
fn active_login_manager_and_presence_match_python() {
    use rexon_core::system_plan::{
        active_login_manager_from, display_manager_present_from,
    };
    let fx = recovery_fx();
    for c in fx["login_manager"]["cases"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let states = c["states"].as_object().unwrap();
        let systemctl = c["systemctl"].as_bool().unwrap();

        // Пробы записываем, чтобы сверить ленивость (порядок и число).
        let probes = std::cell::RefCell::new(Vec::<String>::new());
        let is_enabled = |u: &str| {
            probes.borrow_mut().push(u.to_string());
            states.get(u).and_then(Value::as_str).map(String::from)
        };
        // Отсутствие поля = цель симлинка жива (старые случаи сняты с живого
        // Python, у которого такого различия не было).
        let target_exists = c["target_exists"].as_bool().unwrap_or(!c["symlink"].is_null());
        let got = active_login_manager_from(
            opt_str(&c["symlink"]), target_exists, systemctl, &is_enabled,
        );
        assert_eq!(json!(got), c["active"], "active_login_manager: {note}");
        assert_eq!(
            json!(probes.into_inner()), c["active_probes"],
            "active_login_manager: {note} — порядок проб is-enabled"
        );

        let probes = std::cell::RefCell::new(Vec::<String>::new());
        let is_enabled = |u: &str| {
            probes.borrow_mut().push(u.to_string());
            states.get(u).and_then(Value::as_str).map(String::from)
        };
        let present = display_manager_present_from(
            opt_str(&c["symlink"]), target_exists, systemctl, &is_enabled,
        );
        assert_eq!(json!(present), c["present"], "display_manager_present: {note}");
        assert_eq!(
            json!(probes.into_inner()), c["present_probes"],
            "display_manager_present: {note} — порядок проб is-enabled"
        );
    }
}

/// Висячий симлинк display-manager.service (после `pacman -R sddm`, в том
/// числе нашим `remove_de` для KDE/LXQt) — НЕ экран входа: идиот-защита
/// обязана ставить SDDM, иначе после перезагрузки входа в графику нет. Голден
/// держит этот случай отдельно, чтобы он не выпал при пересъёмке.
#[test]
fn dangling_display_manager_symlink_is_not_a_login_manager() {
    use rexon_core::system_plan::{
        active_login_manager_from, display_manager_present_from, dm_symlink_unit,
    };
    let fx = recovery_fx();
    let dangling: Vec<&Value> = fx["login_manager"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| !c["symlink"].is_null() && c["target_exists"] == json!(false))
        .collect();
    assert!(dangling.len() >= 2, "голден обязан держать висячий симлинк");
    let target = "/usr/lib/systemd/system/sddm.service";
    assert_eq!(dm_symlink_unit(Some(target), true).as_deref(), Some("sddm"));
    assert_eq!(dm_symlink_unit(Some(target), false), None, "висячий → симлинка нет");
    let nothing = |_: &str| None;
    assert!(!display_manager_present_from(Some(target), false, true, &nothing));
    assert_eq!(active_login_manager_from(Some(target), false, true, &nothing), "sddm");
    // Был бы висячий симлинк живым — защита промолчала бы:
    assert!(display_manager_present_from(Some(target), true, true, &nothing));
}

/// Живая машина: активный экран входа — plasmalogin (симлинк на его юнит), и
/// DM в системе ЕСТЬ (значит идиот-защита ensure_display_manager ничего
/// доустанавливать не станет).
#[test]
fn live_machine_is_on_plasma_login_manager() {
    use rexon_core::system_plan::dm_symlink_unit;
    let fx = recovery_fx();
    let lm = &fx["login_manager"];
    let unit = dm_symlink_unit(
        opt_str(&lm["live_symlink"]), lm["live_target_exists"].as_bool().unwrap(),
    );
    assert_eq!(unit.as_deref(), Some("plasmalogin"), "живой активный DM");
    assert_eq!(
        lm["live_states"]["plasmalogin"], "enabled",
        "plasmalogin должен быть enabled на этой машине"
    );
    assert_eq!(lm["cases"][0]["present"], json!(true), "DM на живой машине есть");
}

/// Разбор строк вывода pacman: 50 записанных ответов живого Python (все ветки
/// парсера плюс краевые случаи — многоточие внутри имени, регистр, пустой
/// хвост, счётчик с пробелами) обязаны совпасть с ядром поле в поле.
///
/// Эталон снят ДО удаления Python-копии: `golden/system_plan_texts.json`,
/// ключ `pacman_progress`.
///
/// ⚠️ Хвост списка (12 случаев) с живого Python НЕ снимался — его там и не
/// было: это новые ветки, заведённые после разбора установки Budgie и LXQt
/// у Wehrwolfmann. Три класса, и каждый закреплён нарочно:
///   * переживаемый сбой зеркала («failed retrieving file … from <host>»,
///     «too many errors from <host>») — этап `mirror`, БЕЗ красного креста;
///   * настоящий отказ загрузки («failed to retrieve some files», «failed to
///     commit transaction») — по-прежнему `error` с крестом. Мерило простое:
///     транзакция кончилась удачей → крестов в журнале нет вовсе;
///   * два интерактивных вопроса pacman (члены группы и выбор поставщика) —
///     этап `prompt` с ответом, который pacman даёт себе сам, и глухота к
///     строке «Enter a …», у которой смысла для человека нет.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pacman_progress_lines_match_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    use rexon_core::system_plan::parse_pacman_line;
    let fx = texts_fx();
    let cases = fx["pacman_progress"].as_array().unwrap();
    assert!(cases.len() >= 50, "эталон парсера pacman не должен худеть");
    for c in cases {
        let line = c["line"].as_str().unwrap();
        let got = match parse_pacman_line(line) {
            Some(ev) => serde_json::to_value(&ev).unwrap(),
            None => Value::Null,
        };
        assert_eq!(got, c["result"], "разбор строки pacman: {line:?}");
    }
}

/// Идиот-защита экрана входа: 29 записанных прогонов живого Python (все 12
/// семейств × helper есть/нет, плюс «DM уже есть», «не systemd» и обе ветки
/// отказа) обязаны совпасть с планом ядра — те же команды, те же таймауты,
/// те же сообщения.
///
/// ⚠️ Ничего не устанавливается: эталон снят с подменённым subprocess.run,
/// pkexec не вызывался ни разу (правило «system-install пути не трогаем на
/// живой машине»).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ensure_dm_plan_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    use rexon_core::system_plan::{ensure_dm_plan, EnsureDmPlan};
    let fx = texts_fx_live_helpers();
    let cases = fx["ensure_dm"].as_array().unwrap();
    assert_eq!(cases.len(), 29, "эталон идиот-защиты не должен худеть");
    for c in cases {
        let note = format!(
            "{} helper={} systemctl={}",
            c["family"], c["helper"], c["systemctl"]
        );
        let family = PkgFamily::from_value(c["family"].as_str().unwrap()).unwrap();
        let plan = ensure_dm_plan(
            family,
            c["dm_present"].as_bool().unwrap(),
            c["systemctl"].as_bool().unwrap(),
            c["helper"].as_bool().unwrap(),
        );
        let emits = c["emits"].as_array().unwrap();
        let calls = c["calls"].as_array().unwrap();
        match plan {
            EnsureDmPlan::Present { message, code } => {
                assert_eq!(code, "present", "{note}");
                assert_eq!(json!(message), emits[0][1], "{note}");
                assert!(calls.is_empty(), "{note}: ничего не запускается");
            }
            EnsureDmPlan::Skipped { message, code } => {
                assert_eq!(code, "skipped", "{note}");
                assert_eq!(
                    message.map(Value::from).unwrap_or(Value::Null),
                    emits.first().map(|e| e[1].clone()).unwrap_or(Value::Null),
                    "{note}"
                );
                assert!(calls.is_empty(), "{note}: ничего не запускается");
            }
            EnsureDmPlan::Install {
                message,
                fraction,
                install,
                enable,
                done_message,
                done_fraction,
                ..
            } => {
                assert_eq!(json!(message), emits[0][1], "{note}: вступление");
                assert_eq!(fraction, 0.90, "{note}");
                assert_eq!(json!(install.argv), calls[0]["argv"], "{note}: установка");
                // Установка SDDM — скачивание из репозиториев, а не опрос.
                assert_eq!(
                    json!(rexon_core::utils::DOWNLOAD_TIMEOUT_SECS),
                    calls[0]["timeout"],
                    "{note}: таймаут"
                );
                if calls.len() > 1 {
                    assert_eq!(json!(enable.argv), calls[1]["argv"], "{note}: enable");
                    assert_eq!(json!(30u64), calls[1]["timeout"], "{note}: таймаут");
                }
                if c["code"] == "installed" {
                    let last = emits.last().unwrap();
                    assert_eq!(json!(done_message), last[1], "{note}: итог");
                    assert_eq!(json!(done_fraction), last[2], "{note}");
                }
            }
        }
    }
}

/// Итог главного шага пакетной установки: 38 записанных прогонов живого
/// Python (5 семейств × коды возврата, SteamOS, отсутствие polkit-агента,
/// пустой список пакетов) — тексты и строки журнала обязаны совпасть.
///
/// ⭐ Пять прогонов сверяются НЕ на совпадение, а на починку: на неизвестном
/// семействе оригинал падал RuntimeError, потому что готовил подсказку
/// «поставьте руками» ДО ветвления — даже там, где команда не нужна вовсе.
/// Ядро вместо падения объясняет человеку, что произошло.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn batch_main_result_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    use rexon_core::system_plan::batch_main_result;
    let fx = texts_fx();
    let cases = fx["batch_main"].as_array().unwrap();
    assert_eq!(cases.len(), 38);
    let mut fixed = 0;
    for c in cases {
        let family = PkgFamily::from_value(c["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&c["packages"]);
        let res = c["result"].as_object().map(|r| {
            (
                r["rc"].as_i64().unwrap() as i32,
                r["stderr"].as_str().unwrap(),
                r["stdout"].as_str().unwrap(),
            )
        });
        let agent = c["polkit_agent"].as_bool().unwrap();
        let out = batch_main_result(
            res,
            c["main_seen"].as_bool().unwrap(),
            &packages,
            c["steamos"].as_bool().unwrap(),
            family,
            &|| agent,
            &|| None,
        );
        let note = format!("{} {}", c["family"], c["result"]);
        if !c["raised"].is_null() {
            fixed += 1;
            assert!(!out.ok, "{note}: прежнее падение стало отказом");
            assert!(!out.message.is_empty(), "{note}: человеку сказано, что не так");
            continue;
        }
        assert_eq!(json!(out.ok), c["ok"], "{note}");
        assert_eq!(json!(out.message), c["message"], "{note}");
        let logs: Vec<Value> = out
            .logs
            .iter()
            .map(|(s, m)| json!([s, m]))
            .collect();
        assert_eq!(json!(logs), c["logs"], "{note}: журнал");
    }
    assert_eq!(fixed, 5, "починенных прогонов ровно столько, сколько падало");
}

/// AUR-хвост установки: 10 записанных прогонов (SteamOS и обычный путь,
/// успех, полный и частичный провал, «команда не найдена»).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn batch_aur_messages_match_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    use rexon_core::system_plan::{batch_aur_intro, batch_aur_result};
    let fx = texts_fx();
    let cases = fx["batch_aur"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for c in cases {
        let steamos = c["steamos"].as_bool().unwrap();
        let aur = str_vec(&c["aur"]);
        let installed_set = str_vec(&c["installed"]);
        let rc = c["rc"].as_i64();
        let succeeded = rc == Some(0);
        let failed: Vec<String> = aur
            .iter()
            .filter(|p| !installed_set.contains(p))
            .cloned()
            .collect();
        let installed: Vec<String> = aur
            .iter()
            .filter(|p| installed_set.contains(p))
            .cloned()
            .collect();
        let (stage, msg) = batch_aur_intro(steamos, aur.len());
        let mut logs = vec![json!([stage, msg])];
        for (s, m) in batch_aur_result(steamos, succeeded, aur.len(), &failed, &installed, false) {
            logs.push(json!([s, m]));
        }
        assert_eq!(json!(logs), c["logs"], "AUR: steamos={steamos} rc={rc:?}");
    }
}

/// Доли прогресса установки рабочего стола — порт замыкания `_pkg_cb`.
/// Формулы сверены с исходным выражением Python, счётчик берётся из готового
/// сообщения парсера.
#[test]
fn install_de_fraction_matches_python_formula() {
    use rexon_core::system_plan::install_de_fraction;
    let f = |stage: &str, msg: &str| install_de_fraction(stage, msg);
    assert_eq!(f("pkg_install", "📦 (1/5) Установка: kde"), Some(0.05 + 0.60 * 0.2));
    assert_eq!(f("pkg_install", "📦 (5/5) Установка: kde"), Some(0.05 + 0.60));
    assert_eq!(f("hook", "🔧 (2/4) Arming ConditionNeedsUpdate"), Some(0.70 + 0.05 * 0.5));
    assert_eq!(f("pkg_install", "📦 (1/0) Установка: kde"), Some(0.05 + 0.60));
    assert_eq!(f("pkg_install", "без счётчика"), None);
    assert_eq!(f("done_main", "✅ готово"), Some(0.70));
    assert_eq!(f("aur", "🎨 Установка 3 пакетов из AUR…"), Some(0.75));
    assert_eq!(f("aur_done", ""), Some(0.85));
    assert_eq!(f("aur_warn", ""), Some(0.85));
    assert_eq!(f("download", "⬇️  Скачивание: core.db"), None);
}

/// Исполняемые файлы рабочих столов: у КАЖДОГО стола из базы ядра он должен
/// быть — иначе установленный стол вечно показывается отсутствующим.
#[test]
fn every_de_has_an_executable_to_probe() {
    use rexon_core::system::{de_installed_binaries, list_de_ids};
    for de in list_de_ids() {
        let bins = de_installed_binaries(de);
        assert!(!bins.is_empty(), "у стола {de} нет исполняемого файла для пробы");
        for b in bins {
            assert!(!b.contains(' '), "{de}: {b} не похож на имя файла");
        }
    }
    assert_eq!(
        de_installed_binaries("cinnamon-mint"),
        ["cinnamon", "nemo"],
        "Cinnamon подтверждается ещё и nemo"
    );
    assert!(de_installed_binaries("такого-стола-нет").is_empty());
}

/// Помощник AUR ищется только на Arch и в порядке предпочтения.
#[test]
fn aur_helper_pick_matches_python() {
    use rexon_core::system_plan::pick_aur_helper;
    let have = |set: &'static [&'static str]| move |c: &str| set.contains(&c);
    assert_eq!(
        pick_aur_helper(PkgFamily::Arch, &have(&["paru", "yay"])),
        Some("yay".to_string())
    );
    assert_eq!(
        pick_aur_helper(PkgFamily::Arch, &have(&["pikaur", "paru"])),
        Some("paru".to_string())
    );
    assert_eq!(pick_aur_helper(PkgFamily::Arch, &have(&[])), None);
    assert_eq!(pick_aur_helper(PkgFamily::Debian, &have(&["yay"])), None);
}

/// Пост-установка MATE: подписи «Общих задач», карта snap-путей, содержимое
/// gschema-оверрайда и тексты обоих привилегированных скриптов сняты с
/// прежнего Python и обязаны совпасть с ядром байт в байт.
#[test]
fn mate_post_install_texts_match_python() {
    use rexon_core::system_plan::{
        mate_gschema_cleanup_script, mate_layout_fix_snap_paths,
        mate_layouts_stage_script, mate_locale_actions, mate_user_settings_argv,
        MateLook, MATE_GSCHEMA_PATH, MATE_SNAP_MAP,
    };
    let fx = texts_fx();
    let m = &fx["mate_post_install"];

    for (key, value) in m["locale_actions"].as_object().unwrap() {
        let got = mate_locale_actions(&format!("{key}_RU.UTF-8"));
        assert_eq!(
            got,
            Some((key.as_str(), value.as_str().unwrap())),
            "локаль {key}"
        );
    }
    assert_eq!(mate_locale_actions("fr_FR"), None, "чужая локаль — пропуск");

    let snap: Vec<(String, String)> = m["snap_map"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_str().unwrap().to_string(),
                p[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(snap.len(), MATE_SNAP_MAP.len(), "карта snap-путей не худеет");
    let text: String = snap
        .iter()
        .map(|(s, _)| format!("Exec=/var/lib/snapd/desktop/applications/{s}"))
        .collect::<Vec<_>>()
        .join("\n");
    let want: String = snap
        .iter()
        .map(|(_, a)| format!("Exec=/usr/share/applications/{a}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(mate_layout_fix_snap_paths(&text), want, "snap-пути");

    assert_eq!(json!(MATE_GSCHEMA_PATH), m["gschema_path"]);
    // Оформление пишется В СЛОЙ ПОЛЬЗОВАТЕЛЯ и только ПОДТВЕРЖДЁННОЕ.
    // Когда подтверждено всё, чего хотел прежний общесистемный оверрайд,
    // набор значений обязан совпасть с его содержимым ключ-в-ключ: переезд
    // из системы к пользователю не имеет права молча поменять вид стола.
    let full = MateLook {
        gtk_theme: Some("Yaru-MATE-dark".into()),
        marco_theme: Some("Yaru-MATE-dark".into()),
        icon_theme: Some("Yaru-MATE".into()),
        cursor_theme: Some("Yaru".into()),
        ubuntu_font: true,
        notes: Vec::new(),
    };
    let want_pairs: Vec<(String, String)> = m["gschema_content"]
        .as_str()
        .unwrap()
        .lines()
        .filter(|l| l.contains('=') && !l.starts_with('#') && !l.starts_with('['))
        .map(|l| {
            let (k, v) = l.split_once('=').unwrap();
            (k.trim().to_string(), v.trim().trim_matches('\'').to_string())
        })
        .collect();
    let got_pairs: Vec<(String, String)> = mate_user_settings_argv(&full)
        .into_iter()
        .map(|a| (a[3].clone(), a[4].clone()))
        .collect();
    assert_eq!(got_pairs, want_pairs, "ключи оформления MATE не растеряны");
    // Скрипт уборки общесистемного следа прошлых версий.
    let cleanup = mate_gschema_cleanup_script();
    assert!(cleanup.contains(MATE_GSCHEMA_PATH), "уборка бьёт по оверрайду");
    assert!(cleanup.contains("glib-compile-schemas"), "схемы пересобираются");

    let ls = &m["layouts_script"];
    assert_eq!(
        json!(mate_layouts_stage_script(
            ls["dst"].as_str().unwrap(),
            &str_vec(&ls["files"])
        )),
        ls["text"],
        "скрипт раскладок панели"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Оформление AeroShell: два новых вопроса к движку пакетов (версия
// установленного пакета и список его файлов), ожидание замка и пересборка.
//
// Сырьё для Arch — с ЖИВОЙ машины (`pacman -Q` / `pacman -Ql`, только
// чтение), оракул для версии и списка файлов — ПРЕЖНИЙ Python
// (`_pacman_version` / `_pacman_file_list` до переноса), воспроизведённый
// генератором дословно. Вывод dpkg/rpm синтетический — этих менеджеров на
// машине нет, помечено в `note` каждого случая.
//
// ⛔ Тесты только читают фикстуры: ни pacman, ни paru, ни pkexec.
// ══════════════════════════════════════════════════════════════════════════

fn aeroshell_fx() -> Value {
    recovery_fx()["aeroshell"].clone()
}

/// Команда за версией УСТАНОВЛЕННОГО пакета (не «установлен ли» и не версия
/// в репозиториях — третий, самостоятельный вопрос к движку).
#[test]
fn installed_version_query_matches_golden() {
    for c in aeroshell_fx()["installed_version_query"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = installed_version_query(fam(&c["family"]), c["package"].as_str().unwrap());
        assert_eq!(json!(got), c["query"], "версия-запрос: {note}");
    }
}

/// Разбор вывода: две формы ответа (`имя версия` у pacman, голая версия у
/// dpkg-query/rpm) и «не знаю» на всём, что версией не является.
#[test]
fn parse_installed_version_matches_golden() {
    for c in aeroshell_fx()["installed_version"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = parse_installed_version(
            fam(&c["family"]),
            c["ok"].as_bool().unwrap(),
            c["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got), c["result"], "версия-разбор: {note}");
    }
}

/// Команда за списком файлов пакета.
#[test]
fn package_files_query_matches_golden() {
    for c in aeroshell_fx()["package_files_query"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = package_files_query(fam(&c["family"]), c["package"].as_str().unwrap());
        assert_eq!(json!(got), c["query"], "файлы-запрос: {note}");
    }
}

/// Пустой список файлов при нулевом коде — законный ответ метапакета, а не
/// ошибка: `pacman -Ql base` печатает ноль байт (проверено на живой машине).
#[test]
fn package_files_output_matches_golden() {
    for c in aeroshell_fx()["package_files_output"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = package_files_output(
            c["ok"].as_bool().unwrap(),
            c["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got), c["result"], "файлы-исход: {note}");
    }
}

/// Разбор путей — общий на весь проект, две формы вывода.
#[test]
fn package_file_paths_matches_golden() {
    for c in aeroshell_fx()["package_file_paths"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = package_file_paths(fam(&c["family"]), c["output"].as_str().unwrap());
        assert_eq!(json!(got), c["result"], "файлы-пути: {note}");
    }
}

/// Шаг ожидания замка: тот же цикл, что у установки пакетов.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn lock_wait_step_matches_golden() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in aeroshell_fx()["lock_wait"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = lock_wait_step(
            c["locked"].as_bool().unwrap(),
            c["elapsed_secs"].as_f64().unwrap(),
            c["timeout_secs"].as_u64().unwrap(),
            c["notified"].as_bool().unwrap(),
        );
        assert_eq!(json!(got), c["step"], "замок: {note}");
    }
}

/// Цикл ожидания замка: он крутится в ядре, а не в шиме.
///
/// ⚠️ Ни одной настоящей секунды сна: побочные действия подставлены, поэтому
/// проверяется именно ПОРЯДОК (сколько раз спросили ФС, сколько раз поспали,
/// сказали ли человеку РОВНО один раз), а не длительность.
#[test]
fn wait_for_pkg_lock_turns_the_loop_in_the_core() {
    struct Fake {
        /// Ответы `any_locked` по порядку оборотов.
        locked: Vec<bool>,
        /// Показания часов по порядку опросов.
        clock: Vec<f64>,
        turn: usize,
        tick: usize,
        slept: Vec<u64>,
        said: Vec<(String, String)>,
    }
    impl LockWaitIo for Fake {
        fn any_locked(&mut self, paths: &[String]) -> bool {
            assert!(!paths.is_empty(), "пустой список замков спрашивать незачем");
            let v = self.locked[self.turn.min(self.locked.len() - 1)];
            self.turn += 1;
            v
        }
        fn elapsed_secs(&mut self) -> f64 {
            let v = self.clock[self.tick.min(self.clock.len() - 1)];
            self.tick += 1;
            v
        }
        fn sleep_secs(&mut self, secs: u64) {
            self.slept.push(secs);
        }
        fn notify(&mut self, stage: &str, message: &str) {
            self.said.push((stage.to_string(), message.to_string()));
        }
    }
    let paths = vec!["/var/lib/pacman/db.lck".to_string()];

    // Замок свободен сразу: ни сна, ни слов человеку.
    let mut io = Fake { locked: vec![false], clock: vec![0.0], turn: 0, tick: 0,
                        slept: vec![], said: vec![] };
    assert_eq!(wait_for_pkg_lock(&mut io, &paths, 300),
               (true, rexon_core::system_plan::PKG_LOCK_OK.to_string()));
    assert!(io.slept.is_empty() && io.said.is_empty());

    // Занят три оборота, потом освободился: спали трижды по два, сказали ОДИН
    // раз (иначе окно прогресса залило бы одной и той же строкой).
    let mut io = Fake { locked: vec![true, true, true, false],
                        clock: vec![0.0, 2.0, 4.0, 6.0], turn: 0, tick: 0,
                        slept: vec![], said: vec![] };
    let (ok, msg) = wait_for_pkg_lock(&mut io, &paths, 300);
    assert!(ok && msg == rexon_core::system_plan::PKG_LOCK_OK);
    assert_eq!(io.slept, vec![2, 2, 2]);
    assert_eq!(io.said.len(), 1, "{:?}", io.said);
    assert_eq!(io.said[0].0, rexon_core::system_plan::PKG_LOCK_WAIT_STAGE);

    // Не дождались: отказ с текстом ядра, и он НЕ равен «свободен».
    let mut io = Fake { locked: vec![true], clock: vec![0.0, 300.0], turn: 0,
                        tick: 0, slept: vec![], said: vec![] };
    let (ok, msg) = wait_for_pkg_lock(&mut io, &paths, 300);
    assert!(!ok && msg != rexon_core::system_plan::PKG_LOCK_OK, "{msg}");

    // Семейство без замка (`paths` пуст): ФС не спрашиваем вовсе.
    struct NoFs;
    impl LockWaitIo for NoFs {
        fn any_locked(&mut self, _p: &[String]) -> bool {
            panic!("у семейства без замка спрашивать ФС незачем")
        }
        fn elapsed_secs(&mut self) -> f64 { 0.0 }
        fn sleep_secs(&mut self, _s: u64) { panic!("спать незачем") }
        fn notify(&mut self, _s: &str, _m: &str) { panic!("говорить нечего") }
    }
    assert_eq!(wait_for_pkg_lock(&mut NoFs, &[], 300).0, true);
}

/// Строка растущего журнала: что показать человеку, а что проглотить.
///
/// Читателей было два — установка из репозиториев и сборка из AUR, — и
/// различались они ровно этим. Движок один, различие здесь.
#[test]
fn log_tail_line_splits_progress_from_noise() {
    use LogTailKind::{Aur, Pacman};
    // Установка: разобранная строка отдаёт стадию, неразобранная — «raw»
    // (иначе человек не увидел бы предупреждений менеджера вовсе).
    let (stage, msg) = log_tail_line(Pacman, "(3/9) installing mate-panel...").unwrap();
    assert_ne!(stage, "raw", "{stage}/{msg}");
    assert_eq!(
        log_tail_line(Pacman, "совсем чужая строка"),
        Some(("raw".to_string(), "совсем чужая строка".to_string())),
        "неразобранную строку обязаны показать как есть — иначе человек не \
         увидит того, чего движок ещё не знает"
    );
    // Служебная метка кода возврата — для скрипта, не для человека.
    assert_eq!(log_tail_line(Pacman, "__REXON_EXIT__=0"), None);
    assert_eq!(log_tail_line(Pacman, "   "), None);
    // Хвостовые пробелы срезаются, ведущие — нет: отступ вывода менеджера
    // несёт смысл.
    assert_eq!(log_tail_line(Pacman, "  x  ").unwrap().1, "  x");

    // Сборка: показываем только стадии, иначе десятки тысяч строк компилятора
    // зальют окно прогресса.
    assert_eq!(
        log_tail_line(Aur, "==> Making package: mate-tweak"),
        Some(("aur".to_string(), "    ==> Making package: mate-tweak".to_string()))
    );
    assert_eq!(log_tail_line(Aur, "gcc -O2 -c foo.c"), None);
    assert_eq!(log_tail_line(Aur, ""), None);
    for marker in AUR_PROGRESS_MARKERS {
        assert!(log_tail_line(Aur, &format!("x{marker}y")).is_some(), "{marker}");
    }
}

/// Живое чтение растущего файла: строки доходят по ходу дела, а хвост без
/// перевода строки — в самом конце.
#[test]
fn tail_log_reads_a_growing_file() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    let path = std::env::temp_dir().join(format!("rexon_tail_check_{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut f = std::fs::File::create(&path).unwrap();

    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut tail = tail_log(path.to_str().unwrap(), LogTailKind::Aur, move |s, m| {
        sink.lock().unwrap().push((s.to_string(), m.to_string()));
    });

    writeln!(f, "==> Making package: раз").unwrap();
    writeln!(f, "gcc шум").unwrap();
    f.flush().unwrap();
    // Ждём оборот читателя — но исход проверяем ФАКТОМ, а не сном: строка
    // должна дойти ДО остановки, иначе «живой ход дела» им не является.
    // Бюджет считаем от периода опроса читателя (Aur — 300 мс), а не на глаз:
    // когда весь набор идёт разом, поток-читатель просыпается с задержкой и
    // одного оборота ему не хватает. Зелёный прогон всё равно уходит по факту.
    let mut waited = 0;
    while seen.lock().unwrap().is_empty() && waited < 300 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        waited += 1;
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "строка не дошла до человека, пока установка ещё идёт"
    );

    // Хвост без перевода строки: он существует всегда — процесс убивают или
    // он падает на полуслове.
    write!(f, "==> Хвост без перевода").unwrap();
    f.flush().unwrap();
    let shown = tail.stop();
    let got = seen.lock().unwrap().clone();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[1].1.contains("Хвост без перевода"), "{got:?}");
    assert_eq!(shown.len(), 2, "{shown:?}");
    // Повторная остановка безопасна (шим зовёт её из finally).
    assert!(tail.stop().is_empty());
    let _ = std::fs::remove_file(&path);
}

/// План пересборки: порядок отказов и команда сборщика.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn rebuild_plan_matches_golden() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in aeroshell_fx()["rebuild_plan"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = rebuild_plan(
            &str_vec(&c["packages"]),
            c["helper"].as_str(),
            c["chroot"].as_bool().unwrap(),
            c["polkit_ok"].as_bool().unwrap(),
            c["polkit_missing"].as_str().unwrap(),
        );
        assert_eq!(json!(got), c["plan"], "план пересборки: {note}");
    }
}

/// Строка вывода сборщика: что показывать и что пропускать.
#[test]
fn rebuild_line_matches_golden() {
    for c in aeroshell_fx()["rebuild_line"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        assert_eq!(
            json!(rebuild_line(c["raw"].as_str().unwrap())),
            c["result"],
            "строка сборки: {note}"
        );
    }
}

/// Итог пересборки по коду возврата и хвосту вывода.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn rebuild_outcome_matches_golden() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    for c in aeroshell_fx()["rebuild_outcome"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let got = rebuild_outcome(c["code"].as_i64().unwrap() as i32, &str_vec(&c["tail"]));
        assert_eq!(json!(got), c["outcome"], "итог пересборки: {note}");
    }
}

/// Отдельным тестом — починка, которую нельзя потерять при пересъёмке
/// голдена: человеку больше не показывают «1 пакет(ов)».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn rebuild_start_message_declines_package_word() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let one = rebuild_plan(&["a".to_string()], Some("paru"), false, true, "");
    let two = rebuild_plan(
        &["a".to_string(), "b".to_string()],
        Some("paru"),
        false,
        true,
        "",
    );
    let five: Vec<String> = (0..5).map(|i| format!("p{i}")).collect();
    let five = rebuild_plan(&five, Some("paru"), false, true, "");
    assert_eq!(one.start_message, "🔧 Пересборка 1 пакета оформления…");
    assert_eq!(two.start_message, "🔧 Пересборка 2 пакетов оформления…");
    assert_eq!(five.start_message, "🔧 Пересборка 5 пакетов оформления…");
    for m in [one, two, five] {
        assert!(!m.start_message.contains("(ов)"), "{}", m.start_message);
    }
}

/// Тексты пересборки AeroShell по-русски — дословно как до перевода на
/// каталог (снимок `ca42e74^`: константы `REBUILD_*` и `format!`).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn aeroshell_russian_texts_as_before() {
    use rexon_core::aeroshell as a;
    let _lang = i18n_pin::ru();
    assert_eq!(a::rebuild_nothing(), "Нечего пересобирать");
    assert_eq!(
        a::rebuild_no_helper(),
        "Не найден сборщик пакетов AUR (paru или yay) — без него пересобрать оформление нельзя"
    );
    assert_eq!(a::rebuild_no_command(), "Не удалось составить команду пересборки");
    assert_eq!(a::rebuild_done_message(), "✅ Оформление пересобрано");
    assert_eq!(
        a::rebuild_success_message(),
        "Оформление пересобрано под текущую версию системы.\nЧтобы всё встало на место, \
         выйдите из системы и войдите снова."
    );
    assert_eq!(a::rebuild_spawn_failure("yay", "нет файла"), "Не удалось запустить yay: нет файла");
    assert_eq!(a::rebuild_exit_code_message(2), "Сборка завершилась с кодом 2");
    assert_eq!(
        a::rebuild_busy_message(),
        "⏳ Оформление уже проверяется — дождитесь, пока закончится."
    );
    assert_eq!(a::survey_running_message(), "🔍 Проверяю оформление…");
    assert_eq!(
        a::rebuild_timeout_message(),
        "Сборка оформления шла слишком долго и была остановлена. Попробуйте ещё раз позже."
    );
    assert_eq!(a::rebuild_failed_head(), "Пересобрать оформление не удалось.");
    assert_eq!(a::rebuild_tail_head(), "Последние строки сборки:");
    let denied = a::rebuild_denied_message();
    assert!(
        denied.starts_with(
            "Пароль администратора не подтверждён — пересборка остановлена.\nНажмите «"
        ),
        "{denied}"
    );
    assert!(denied.ends_with("» в настройках Rexon, когда будете готовы ввести пароль."), "{denied}");
}

/// Те же тексты по-английски — без русских букв, фразами.
#[test]
fn aeroshell_english_texts_have_no_russian() {
    use rexon_core::aeroshell as a;
    let _lang = i18n_pin::en();
    let texts = [
        a::rebuild_nothing(),
        a::rebuild_no_helper(),
        a::rebuild_no_command(),
        a::rebuild_done_message(),
        a::rebuild_success_message(),
        a::rebuild_start_message(1),
        a::rebuild_start_message(3),
        a::rebuild_spawn_failure("yay", "E"),
        a::rebuild_exit_code_message(2),
        a::rebuild_busy_message(),
        a::survey_running_message(),
        a::rebuild_timeout_message(),
        a::rebuild_failed_head(),
        a::rebuild_tail_head(),
        a::rebuild_denied_message(),
    ];
    for t in &texts {
        assert!(!t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')), "{t}");
    }
    assert_eq!(a::rebuild_start_message(1), "🔧 Rebuilding 1 appearance package…");
    assert_eq!(a::rebuild_start_message(3), "🔧 Rebuilding 3 appearance packages…");
    let denied = a::rebuild_denied_message();
    assert!(denied.contains("Click “"), "{denied}");
    assert!(!denied.contains("“ "), "кнопка без значка впереди: {denied}");
}

/// И вторая починка: у метапакета `pacman -Ql` печатает ноль байт с кодом 0,
/// и это «файлов нет», а не «спросить не вышло». Различие держит вся сверка
/// оформления: пакет без файлов не должен объявляться собранным.
#[test]
fn empty_file_list_is_an_answer_not_a_failure() {
    assert_eq!(package_files_output(true, ""), Some(String::new()));
    assert_eq!(package_files_output(false, ""), None);
    assert!(package_file_paths(PkgFamily::Arch, "").is_empty());
}

/// Одно решение «экран входа рисует Plasma Login Manager» на продукт.
#[test]
fn plasma_login_is_recognised_by_unit_name() {
    use rexon_core::system_plan::{is_plasma_login, PLASMA_LOGIN_UNIT};
    assert_eq!(PLASMA_LOGIN_UNIT, "plasmalogin");
    assert!(is_plasma_login("plasmalogin"));
    assert!(is_plasma_login("plasmalogin\n"));
    for other in ["sddm", "gdm", "lightdm", "cosmic-greeter", "", "plasmalogin-greeter"] {
        assert!(!is_plasma_login(other), "{other}");
    }
}

// ── Английский интерфейс ────────────────────────────────────────────────

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Всё, что движок установки говорит человеку (отказы, журнал хода,
/// итоги, ожидание замка, откат форков, сброс Plymouth и SDDM, доводка
/// MATE), при английском языке — без единой русской буквы.
///
/// Не собираются намеренно: тексты привилегированных помощников и polkit
/// (их ведёт заход «один пароль») и подписи шагов плана (служебные метки
/// для сверки с Python, человеку не показываются).
#[test]
fn english_system_plan_has_no_russian() {
    use rexon_core::system_plan as sp;
    let _lang = i18n_pin::en();
    let pk = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let mut all: Vec<String> = Vec::new();

    all.push(sp::install_cmd(PkgFamily::Unknown, &pk(&["a"])).unwrap_err());
    all.push(sp::remove_cmd(PkgFamily::Unknown, &pk(&["a"])).unwrap_err());
    all.push(sp::query_cmd(PkgFamily::Unknown, "a").unwrap_err());
    all.push(sp::install_cmd(PkgFamily::Arch, &pk(&["/tmp/x.pkg.tar.zst"])).unwrap_err());
    let ip = sp::install_plan(PkgFamily::Arch, &pk(&["xfce4"]), false, None, false).unwrap();
    all.push(ip.sudo_hint);
    all.push(sp::install_plan(PkgFamily::Arch, &[], false, None, false).unwrap().early_result.unwrap().1);
    for n in ["hyprland", "kde"] {
        all.push(sp::nixos_de_snippet(n).unwrap());
    }
    if let sp::RemoveDeDecision::Refused(m) = sp::remove_de_decision("kde", "KDE Plasma", "kde", false, &pk(&["a"]), &[]) {
        all.push(m);
    }
    if let sp::RemoveDeDecision::Refused(m) = sp::remove_de_decision("xfce", "XFCE", "kde", false, &[], &[]) {
        all.push(m);
    }
    all.extend(sp::switch_de_guard("unknown", "kde", "KDE Plasma"));
    all.extend(sp::switch_de_guard("kde", "kde", "KDE Plasma"));
    match sp::lock_wait_step(true, 0.0, 300, false) {
        sp::LockWaitStep::Notify { message, .. } => all.push(message),
        other => panic!("{other:?}"),
    }
    all.push(sp::pkg_lock_timeout_message(300));
    all.push(sp::plasma_fork_unresolved_message(&[
        sp::UnresolvedFork { fork: "aeroshell-kwin".into(), candidates: vec![] },
        sp::UnresolvedFork { fork: "aeroshell-plasma".into(), candidates: pk(&["plasma-workspace"]) },
    ]));
    all.push(sp::plasma_fork_failure_message("error: x", Some("sudo pacman -S kwin")));
    all.push(sp::plasma_fork_failure_message("", None));
    all.extend([
        sp::plasma_fork_nothing_message(),
        sp::plasma_fork_no_command_message(),
        sp::plasma_fork_none_message(),
        sp::plasma_fork_no_actions_message(),
        sp::plasma_fork_reset_done_message(),
        sp::plymouth_no_target_message(),
        sp::plymouth_no_tool_message(),
        sp::plymouth_reset_failed_message(),
        sp::plymouth_already_default_message("bgrt"),
        sp::plymouth_missing_theme_file_message("bgrt"),
        sp::plymouth_reset_success_message("bgrt"),
        sp::sddm_reset_success_message(&pk(&["10-x.conf"]), Some("breeze")),
        sp::sddm_reset_success_message(&pk(&["10-x.conf"]), None),
        sp::sddm_already_stock_message(),
        sp::sddm_reset_failed_message(),
        sp::pkexec_missing_message(),
        sp::pkexec_denied_message("sudo x"),
    ]);
    for line in [
        "(1/3) installing kwin...",
        "(2/3) upgrading kwin...",
        "(3/3) removing kwin...",
        "(1/1) reinstalling kwin...",
        "(1/1) downgrading kwin...",
        "downloading kwin-6.4.pkg.tar.zst...",
        "Packages (12) a b c",
        "Total Download Size:   12.00 MiB",
        "Total Installed Size:  40.00 MiB",
        ":: There are 24 members in group plasma:",
        ":: There are 2 providers available for ttf-font:",
        "error: failed retrieving file 'x' from mirror.example.org : timeout",
        "warning: too many errors from mirror.example.org, skipping for the remainder of this transaction",
        "warning: kwin-6.4-1 is up to date -- skipping",
        ":: Retrieving packages...",
    ] {
        if let Some(ev) = sp::parse_pacman_line(line) {
            all.push(ev.message);
        }
    }
    for (present, systemctl) in [(true, true), (false, true)] {
        match sp::ensure_dm_plan(PkgFamily::Arch, present, systemctl, false) {
            sp::EnsureDmPlan::Present { message, .. } => all.push(message),
            sp::EnsureDmPlan::Install { message, done_message, .. } => all.extend([message, done_message]),
            sp::EnsureDmPlan::Skipped { message, .. } => all.extend(message),
        }
    }
    if let sp::EnsureDmPlan::Skipped { message, .. } = sp::ensure_dm_plan(PkgFamily::Nix, false, true, false) {
        all.extend(message);
    }
    let agent = || false;
    for r in [None, Some((126, "", "")), Some((1, "", ""))] {
        all.push(sp::batch_main_result(r, true, &pk(&["a"]), false, PkgFamily::Arch, &agent, &|| None).message);
    }
    all.push(sp::batch_main_result(Some((126, "", "")), true, &pk(&["a"]), true, PkgFamily::Arch, &agent, &|| None).message);
    let agent_on = || true;
    all.push(sp::batch_main_result(Some((126, "", "")), true, &pk(&["a"]), false, PkgFamily::Arch, &agent_on, &|| None).message);
    for steamos in [false, true] {
        let ok = sp::batch_main_result(Some((0, "", "")), true, &pk(&["a", "b"]), steamos, PkgFamily::Arch, &agent, &|| None);
        all.extend(ok.logs.into_iter().map(|(_, t)| t));
        all.push(sp::batch_aur_intro(steamos, 3).1);
        all.push(sp::pkg_install_start_message(3, steamos));
        for (ok, timed_out) in [(true, false), (false, true), (false, false)] {
            all.extend(
                sp::batch_aur_result(steamos, ok, 3, &pk(&["x"]), &pk(&["y"]), timed_out)
                    .into_iter()
                    .map(|(_, t)| t),
            );
        }
    }
    all.push(sp::aur_timeout_message(1));
    let look = sp::mate_look(&sp::MateLookInput {
        gtk_themes: pk(&["Adwaita"]),
        marco_themes: vec![],
        icon_themes: vec![],
        cursor_themes: vec![],
        ubuntu_font: false,
    });
    all.extend(look.notes.clone());
    all.extend(sp::mate_gschema_success_lines(&look));
    for errors in [0, 2] {
        let (lines, result) = sp::mate_post_install_summary(errors);
        all.extend(lines);
        all.push(result);
    }
    for (_, title, _) in sp::MATE_EXTRA_PACKAGES {
        all.push(rexon_core::i18n::tr(title));
    }
    let t = sp::extra_packages_texts("yaru", &pk(&["a"]), None);
    all.extend([t.not_arch, t.already, t.no_helper, t.installing, t.done, t.failed, t.failed_timeout]);

    assert!(all.len() > 80, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// Английские фразы, собранные из частей, читаются как фразы.
#[test]
fn english_system_plan_reads_naturally() {
    use rexon_core::system_plan as sp;
    let _lang = i18n_pin::en();
    assert_eq!(sp::pkg_install_start_message(1, false), "📦 Installing 1 package…");
    assert_eq!(sp::pkg_install_start_message(3, true), "📦 SteamOS: installing 3 packages…");
    assert_eq!(
        sp::parse_pacman_line("(1/3) installing kwin...").unwrap().message,
        "📦 (1/3) Installing: kwin"
    );
    assert_eq!(
        sp::parse_pacman_line(":: There are 24 members in group plasma:").unwrap().message,
        "🧩 Group plasma (24 packages) — installing all of it, nothing to answer"
    );
    assert_eq!(
        sp::sddm_reset_success_message(&["10-x.conf".to_string()], Some("breeze")),
        "Removed SDDM overrides: 10-x.conf. The login screen will go back to the distribution \
default “breeze”."
    );
    assert_eq!(
        sp::switch_de_guard("kde", "kde", "KDE Plasma").as_deref(),
        Some("Already in use: KDE Plasma")
    );
    assert_eq!(
        sp::nixos_de_snippet("hyprland").unwrap(),
        "programs.hyprland.enable = true;\n# + set up waybar, wofi, kitty, etc."
    );
}
