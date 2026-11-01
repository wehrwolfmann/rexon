//! Golden-сверка порта `ruffle_manager.rs` с оригиналом
//! `rexon/ruffle_manager.py`.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.ruffle_manager` одноразовым
//! генератором (`Popen`/`subprocess.run` замоканы, реальное скачивание/запуск
//! НЕ исполнялись; пути под временный HOME нормализованы в `/HOME`,
//! `sys.executable` → `/PY`). Покрыто: выбор ассета релиза по архитектуре и
//! его URL, web-URL, пути хранения, выбор бинаря (`get_ruffle_path`), argv
//! прямого запуска SWF и запуска как обои (+ скрипт-обёртка, basename/splitext),
//! выбор источника версии, решение `ensure_installed`, арифметика прогресса.
//!
//! `golden/ruffle_manager_cases.json` — снятые значения.

use rexon_core::ruffle_manager as rm;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/ruffle_manager_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn opt_s(v: &Value) -> Option<String> {
    v.as_str().map(String::from)
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

const HOME: &str = "/HOME";

#[test]
fn arch_normalization() {
    let g = fx();
    for (machine, expected) in g["arch"].as_object().unwrap() {
        assert_eq!(rm::normalize_arch(machine), s(expected), "arch {machine}");
    }
}

#[test]
fn download_urls_and_web() {
    let g = fx();
    let urls = g["download_urls"].as_object().unwrap();
    assert_eq!(rm::download_url("x86_64").as_deref(), Some(s(&urls["x86_64"])));
    assert_eq!(rm::download_url("aarch64").as_deref(), Some(s(&urls["aarch64"])));
    // Неподдержанная архитектура — как в оригинале (нет в DOWNLOAD_URLS).
    assert_eq!(rm::download_url("armv7l"), None);
    assert_eq!(rm::download_url("i686"), None);

    assert_eq!(rm::web_download_url(), s(&g["download_url_web"]));
    assert_eq!(rm::GITHUB_RELEASES_URL, s(&g["github_releases_url"]));
    assert_eq!(rm::EXPECTED_SIZE_MB, g["expected_size_mb"].as_u64().unwrap());
    assert_eq!(
        rm::EXPECTED_SIZE_WEB_MB,
        g["expected_size_web_mb"].as_u64().unwrap()
    );
}

#[test]
fn storage_paths() {
    let g = fx();
    let p = &g["paths"];
    let paths = rm::ruffle_paths(HOME);
    assert_eq!(paths.dir, s(&p["dir"]));
    assert_eq!(paths.bin, s(&p["bin"]));
    assert_eq!(paths.web_dir, s(&p["web_dir"]));
    assert_eq!(paths.version_file, s(&p["version_file"]));
    assert_eq!(paths.web_js, s(&p["web_js"]));
}

#[test]
fn ruffle_path_selection() {
    let g = fx();
    let rp = &g["ruffle_path"];
    let bin = rm::ruffle_paths(HOME).bin;
    // neither
    assert_eq!(rm::ruffle_path(false, false, &bin), opt_s(&rp["neither"]));
    // только системный → буквально "ruffle"
    assert_eq!(
        rm::ruffle_path(false, true, &bin),
        opt_s(&rp["system_only"])
    );
    // локальный бинарь
    assert_eq!(rm::ruffle_path(true, false, &bin), opt_s(&rp["installed"]));
    // есть и пакет, и наша копия → берём пакет дистрибутива
    assert_eq!(rm::ruffle_path(true, true, &bin), opt_s(&rp["both"]));
    // is_installed = isfile && x_ok
    assert!(rm::is_installed(true, true));
    assert!(!rm::is_installed(true, false));
    assert!(!rm::is_installed(false, true));
}

#[test]
fn run_swf_argv_parity() {
    let g = fx();
    let bin = rm::ruffle_paths(HOME).bin;
    let swf = format!("{HOME}/Мои обои/cool.swf");
    assert_eq!(
        rm::run_swf_argv(&bin, &swf, true),
        str_vec(&g["run_swf_argv"]["fullscreen"])
    );
    assert_eq!(
        rm::run_swf_argv(&bin, &swf, false),
        str_vec(&g["run_swf_argv"]["windowed"])
    );
}

#[test]
fn run_swf_as_wallpaper_parity() {
    let g = fx();
    let w = &g["run_swf_as_wallpaper"];
    let paths = rm::ruffle_paths(HOME);
    let bin = &paths.bin;
    let swf = format!("{HOME}/Мои обои/cool.swf");

    let stem = rm::swf_stem(&swf);
    assert_eq!(stem, s(&w["swf_stem"]));

    let wp = rm::wrapper_path(&paths.dir, &stem);
    assert_eq!(wp, s(&w["wrapper_path"]));

    assert_eq!(rm::wrapper_script(bin, &swf), s(&w["wrapper_content"]));

    // ⛔ЗДЕСЬ ЭТАЛОН НЕ ПОВТОРЯЕМ, И ЭТО НЕ НЕБРЕЖНОСТЬ. Оригинал звал
    // `<python> -m rexon.app_wallpaper <обёртка> --mode subprocess`
    // (`w["cmd"]`), а модуль `rexon.app_wallpaper` снят с дерева коммитом
    // `2091eac`: SWF-обои не работали с 23.08.2026 — процесс падал сразу, а
    // окно рапортовало «обои применены». Flash теперь играет НАТИВНО, своим
    // хостом (Ruffle библиотекой, крейт `rexon-swf`).
    let host = "/usr/lib/rexon/rexon-wallpaper-host";
    assert_eq!(
        rm::wallpaper_argv(host, &swf),
        vec![host, "--mode", "swf", swf.as_str()]
    );
    // Одна правда строки запуска: то же самое, что кладёт в план исполнитель.
    assert_eq!(
        rm::wallpaper_argv(host, &swf),
        rexon_core::wallpaper_engine::swf_host_plan(host, &swf)[0].argv
    );
    // Ни интерпретатора, ни модуля, ни bash-обёртки вокруг внешнего Ruffle.
    let cmd = rm::wallpaper_argv(host, &swf).join(" ");
    for mark in ["python", "-m rexon", "app_wallpaper", "subprocess", &wp] {
        assert!(!cmd.contains(mark), "в запуске SWF-обоев осталось «{mark}»: {cmd}");
    }
    // Эталон остаётся описанным — расхождение намеренное и названное вслух.
    assert!(
        str_vec(&w["cmd"]).iter().any(|a| a == "rexon.app_wallpaper"),
        "эталон Python перестал звать снятый модуль — сверку пора убирать целиком"
    );
}

#[test]
fn swf_stem_edge_cases() {
    let g = fx();
    for (path, expected) in g["swf_stem"].as_object().unwrap() {
        assert_eq!(rm::swf_stem(path), s(expected), "stem {path}");
    }
}

#[test]
fn version_resolution() {
    let g = fx();
    let v = &g["version"];
    // cmd_ok: путь есть, rc==0 → stdout.trim()
    assert_eq!(
        rm::resolve_version(Some("/bin/ruffle"), Some("ruffle nightly-2026-07-20\n"), None),
        opt_s(&v["cmd_ok"])
    );
    // file_fallback: путь есть, rc!=0 (cmd None) → содержимое .version, trim
    assert_eq!(
        rm::resolve_version(Some("/bin/ruffle"), None, Some("nightly-2026-07-19\n")),
        opt_s(&v["file_fallback"])
    );
    // none: путь есть, ни cmd, ни файла → None
    assert_eq!(
        rm::resolve_version(Some("/bin/ruffle"), None, None),
        opt_s(&v["none"])
    );
    // no_path_ignores_file: пути нет → None (файл НЕ читается)
    assert_eq!(
        rm::resolve_version(None, None, Some("nightly-2026-07-18\n")),
        opt_s(&v["no_path_ignores_file"])
    );

    let vf = &g["version_file_content"];
    assert_eq!(rm::version_file_content(s(&vf["date"])), s(&vf["content"]));
    assert_eq!(rm::parse_version_file("nightly-2026-07-19\n"), "nightly-2026-07-19");
}

#[test]
fn ensure_decision_truth_table() {
    let g = fx();
    for case in g["ensure"].as_array().unwrap() {
        let inst = case["is_installed"].as_bool().unwrap();
        let sysi = case["is_system_installed"].as_bool().unwrap();
        let web = case["is_web_installed"].as_bool().unwrap();
        let d = rm::ensure_decision(inst, sysi, web);
        assert_eq!(d.need_bin, case["need_bin"].as_bool().unwrap(),
                   "need_bin {inst},{sysi},{web}");
        assert_eq!(d.need_web, case["need_web"].as_bool().unwrap(),
                   "need_web {inst},{sysi},{web}");
        // already_ok = ранний return True (Python result всегда True в стабах)
        assert!(case["result"].as_bool().unwrap());
    }
}

#[test]
fn progress_pct_parity() {
    let g = fx();
    for case in g["progress_pct"].as_array().unwrap() {
        let d = case["downloaded"].as_u64().unwrap();
        let t = case["total"].as_u64().unwrap();
        assert_eq!(
            rm::download_progress_pct(d, t),
            case["pct"].as_u64().unwrap(),
            "pct {d}/{t}"
        );
    }
}

#[test]
fn progress_bar_parity() {
    let g = fx();
    for case in g["progress_bar"].as_array().unwrap() {
        let pct = case["pct"].as_u64().unwrap();
        assert_eq!(rm::progress_bar(pct), s(&case["bar"]), "bar {pct}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Закрытие дыр паритета: выбор системного бинаря (имена пакетов разных
//  дистрибутивов) и предикат «копировать рядом с бинарём». Эталоны сняты с
//  живого `RuffleManager.system_bin` при Python-путь до его удаления с подменённым
//  `shutil.which` (в PATH ничего не искалось).
// ═══════════════════════════════════════════════════════════════════════════

/// Список имён бинаря Ruffle по дистрибутивам и его ПОРЯДОК — как в оригинале
/// (`SYSTEM_BINS`): порядок определяет, какой пакет выиграет.
#[test]
fn system_bin_names_match_python() {
    assert_eq!(str_vec(&fx()["system_bin_names"]), rm::SYSTEM_BINS.to_vec());
}

/// `system_bin`: первое найденное имя из `SYSTEM_BINS`, иначе None. Возвращается
/// РЕАЛЬНЫЙ путь пакета (в разных системах бинарь называется по-разному).
#[test]
fn system_bin_selection_matches_python() {
    for c in fx()["system_bin"].as_array().unwrap() {
        let which = c["which"].as_object().unwrap();
        let got = rm::select_system_bin(|name| {
            which
                .get(name)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
        assert_eq!(got, opt_s(&c["expected"]), "which={which:?}");
    }
}

/// Предикат копирования лицензий/README рядом с бинарём (регистр значим).
#[test]
fn extra_asset_predicate_matches_python() {
    for (name, want) in fx()["extra_asset"].as_object().unwrap() {
        assert_eq!(
            rm::is_extra_asset(name),
            want.as_bool().unwrap(),
            "is_extra_asset({name})"
        );
    }
}
