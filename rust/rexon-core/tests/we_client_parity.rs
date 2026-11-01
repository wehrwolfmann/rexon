//! Паритет порта `rexon/qt_wallpaper_engine/client.py` с живым Python.
//!
//! Эталон `golden/we_client_cases.json` снят с САМОГО Python: `QWebSocket`
//! подменён заглушкой (перехват `sendBinaryMessage`), `os.path.isfile` /
//! `os.access` / `shutil.which` / `subprocess.run` замоканы, живой демон НЕ
//! дёргался. В фикстуре — hex кадров `Request`, порядок проверок ФС при
//! поиске бинаря, содержимое юнита/`.desktop`, тексты сообщений и вердикты
//! разбора `busctl`.
//!
//! Байтовая сверка запросов — один-в-один. Исключение — `settings_set`:
//! у него `map`-поле (`plugins`), порядок записей на проводе не определён ни
//! у `google.protobuf`, ни у prost → сверка структурная (декодом).

use prost::Message;
use serde_json::Value;
use std::collections::HashMap;

use rexon_core::proto::control as pb;
use rexon_core::waywallen_client::{DbusReply, DbusValue, WaywallenIo};
use rexon_core::we_client as wc;

mod i18n_pin;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/we_client_cases.json")).unwrap()
}

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Каталог пакета `rexon/` в эталоне заменён плейсхолдером (эталон переносим).
const PKG: &str = "<PKG>";
const HOME: &str = "/home/tester";

// ============================================================================
// 1. Поиск бинаря демона
// ============================================================================

/// ⚠️ ГЛАВНЫЙ тест блока: порядок проверок значим (фикс бага 2026-06-24 —
/// хватался устаревший извлечённый бинарь owe-ref и демон падал на миграции
/// БД движка). Последовательность сверяется с ЖИВЫМ Python: там записаны
/// реальные обращения `os.path.isfile` / `shutil.which`.
#[test]
fn порядок_кандидатов_бинаря() {
    let g = golden();
    let expect: Vec<(String, String)> = g["find_daemon_probe_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r[0].as_str().unwrap().to_string(),
                r[1].as_str().unwrap().to_string(),
            )
        })
        .collect();

    let got: Vec<(String, String)> = wc::daemon_binary_candidates(PKG, HOME)
        .into_iter()
        .map(|c| match c {
            wc::DaemonCandidate::File(p) => ("isfile".to_string(), p),
            wc::DaemonCandidate::PathLookup(n) => ("which".to_string(), n),
        })
        .collect();

    assert_eq!(got, expect, "порядок проверок разошёлся с Python");
}

/// Устаревший извлечённый бинарь исключён сознательно — он не должен
/// появиться в кандидатах ни при каких условиях.
#[test]
fn owe_ref_не_в_кандидатах() {
    for c in wc::daemon_binary_candidates(PKG, HOME) {
        if let wc::DaemonCandidate::File(p) = c {
            assert!(
                !p.contains("owe-ref"),
                "устаревший извлечённый бинарь вернулся в список: {p}"
            );
        }
    }
    let g = golden();
    let fallbacks: Vec<String> = g["daemon_fallbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(fallbacks, wc::DAEMON_FALLBACKS.to_vec());
}

struct FakeFs {
    exec_files: Vec<String>,
    which: Option<String>,
}

impl wc::DaemonFs for FakeFs {
    fn is_exec_file(&mut self, path: &str) -> bool {
        self.exec_files.iter().any(|p| p == path)
    }
    fn which(&mut self, _name: &str) -> Option<String> {
        self.which.clone()
    }
}

#[test]
fn выбор_бинаря_по_приоритету() {
    let bundled = format!("{PKG}/vendor/waywallen.AppImage");
    let bundled_plain = format!("{PKG}/vendor/waywallen");
    let managed = wc::managed_appimage_path(HOME);
    let local_bin = format!("{HOME}/.local/bin/waywallen");
    let dev = format!("{HOME}/Projects/waywallen/waywallen.AppImage");

    // Всё есть → побеждает вшитый AppImage.
    let mut fs = FakeFs {
        exec_files: vec![
            bundled.clone(),
            bundled_plain.clone(),
            managed.clone(),
            local_bin.clone(),
            dev.clone(),
        ],
        which: Some("/usr/bin/waywallen".into()),
    };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), Some(bundled));

    // Вшитого нет — второе имя внутри vendor/.
    let mut fs = FakeFs {
        exec_files: vec![bundled_plain.clone(), managed.clone()],
        which: Some("/usr/bin/waywallen".into()),
    };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), Some(bundled_plain));

    // vendor/ пуст → управляемый AppImage важнее PATH.
    let mut fs = FakeFs {
        exec_files: vec![managed.clone(), local_bin.clone()],
        which: Some("/usr/bin/waywallen".into()),
    };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), Some(managed));

    // Управляемого нет → PATH.
    let mut fs = FakeFs {
        exec_files: vec![local_bin.clone(), dev.clone()],
        which: Some("/usr/bin/waywallen".into()),
    };
    assert_eq!(
        wc::find_daemon_binary(&mut fs, PKG, HOME),
        Some("/usr/bin/waywallen".to_string())
    );

    // PATH пуст → единственный запасной путь.
    let mut fs =
        FakeFs { exec_files: vec![local_bin.clone(), dev.clone()], which: None };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), Some(local_bin));

    // ⚠ dev-сборка из списка кандидатов убрана (файла больше нет): сама по
    // себе она демоном больше не считается.
    let mut fs = FakeFs { exec_files: vec![dev.clone()], which: None };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), None);

    // Ничего нет.
    let mut fs = FakeFs { exec_files: vec![], which: None };
    assert_eq!(wc::find_daemon_binary(&mut fs, PKG, HOME), None);
}

// ── Движок из пакета (28.09.2026) ──────────────────────────────────────────

/// ФС с версиями движков — для выбора «пакет или управляемый».
struct VersionedFs {
    exec_files: Vec<String>,
    versions: HashMap<String, String>,
    asked: Vec<String>,
}

impl wc::DaemonFs for VersionedFs {
    fn is_exec_file(&mut self, path: &str) -> bool {
        self.exec_files.iter().any(|p| p == path)
    }
    fn which(&mut self, _name: &str) -> Option<String> {
        None
    }
    fn binary_version(&mut self, path: &str) -> Option<String> {
        self.asked.push(path.to_string());
        self.versions.get(path).cloned()
    }
}

/// Движок пакета стоит сразу за переопределением разработчика
/// (`rexon/vendor/`) и перед управляемым AppImage.
#[test]
fn packaged_engine_after_vendor_before_managed() {
    const ENGINE: &str = "/opt/test-engine";
    let vendor = format!("{PKG}/vendor/waywallen.AppImage");
    let packaged = wc::engine_appimage_path(ENGINE);
    let managed = wc::managed_appimage_path(HOME);
    assert_eq!(packaged, "/opt/test-engine/waywallen.AppImage");
    assert_eq!(
        wc::engine_appimage_path(wc::ENGINE_DIR),
        "/usr/lib/rexon/engine/waywallen.AppImage",
        "путь движка пакета обязан совпадать с PKGBUILD"
    );

    let cands: Vec<String> = wc::daemon_binary_candidates_in(PKG, HOME, ENGINE)
        .into_iter()
        .map(|c| match c {
            wc::DaemonCandidate::File(p) | wc::DaemonCandidate::PathLookup(p) => p,
        })
        .collect();
    let at = |p: &str| cands.iter().position(|c| c == p).unwrap();
    assert!(at(&vendor) < at(&packaged) && at(&packaged) < at(&managed), "{cands:?}");

    // vendor есть → он (переопределение разработчика).
    let mut fs = FakeFs {
        exec_files: vec![vendor.clone(), packaged.clone(), managed.clone()],
        which: None,
    };
    assert_eq!(wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE), Some(vendor));

    // vendor нет, пакет есть, управляемого нет → пакет, версии не спрашиваем.
    let mut fs = VersionedFs {
        exec_files: vec![packaged.clone()],
        versions: HashMap::new(),
        asked: vec![],
    };
    assert_eq!(
        wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE),
        Some(packaged.clone())
    );
    assert!(fs.asked.is_empty(), "без второго движка AppImage не запускаем: {:?}", fs.asked);
}

/// Оба движка на диске: новее — управляемый (он уже переписал базу под
/// себя, старый пакетный на ней не стартует); иначе — пакет.
#[test]
fn packaged_or_managed_by_version() {
    const ENGINE: &str = "/opt/test-engine";
    let packaged = wc::engine_appimage_path(ENGINE);
    let managed = wc::managed_appimage_path(HOME);
    let pick = |b: Option<&str>, m: Option<&str>| {
        let mut versions = HashMap::new();
        if let Some(b) = b {
            versions.insert(packaged.clone(), b.to_string());
        }
        if let Some(m) = m {
            versions.insert(managed.clone(), m.to_string());
        }
        let mut fs = VersionedFs {
            exec_files: vec![packaged.clone(), managed.clone()],
            versions,
            asked: vec![],
        };
        wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE).unwrap()
    };
    assert_eq!(pick(Some("0.4.2"), Some("0.4.3")), managed, "управляемый новее");
    assert_eq!(pick(Some("0.4.2"), Some("0.10.0")), managed, "сравнение числами, не строкой");
    assert_eq!(pick(Some("0.4.2"), Some("0.4.2")), packaged, "равные — пакет");
    assert_eq!(pick(Some("0.4.3"), Some("0.4.2")), packaged, "пакет новее");
    assert_eq!(pick(Some("0.4.2"), None), packaged, "версия управляемого неизвестна");
    assert_eq!(pick(None, Some("0.4.3")), packaged, "версия пакета неизвестна");

    assert!(wc::prefer_managed_over_bundled(Some("0.4.2"), Some("0.5.0")));
    assert!(!wc::prefer_managed_over_bundled(Some(""), Some("0.5.0")));
}

/// Флаг `--plugin` уходит демону ТОЛЬКО при каталоге плагинов пакета: у
/// waywallen 0.2.x такого флага нет, и лишний аргумент сорвал бы старт.
#[test]
fn launch_args_with_packaged_plugins() {
    assert_eq!(wc::engine_launch_args(None), vec!["--no-ui", "--no-tray"]);
    assert_eq!(
        wc::engine_launch_args(Some("/usr/lib/rexon/engine/")),
        vec!["--no-ui", "--no-tray", "--plugin", "/usr/lib/rexon/engine"]
    );
    assert_eq!(
        wc::engine_plugins_path(wc::ENGINE_DIR),
        "/usr/lib/rexon/engine/plugins",
        "демон ищет плагины в <каталог>/plugins (waywallen v0.4.2)"
    );
    // Прежняя дверь без комплекта не изменилась.
    assert_eq!(wc::daemon_launch_argv("/x/w").1, wc::engine_launch_args(None));

    let st = wc::DaemonStatus::default();
    match wc::ensure_running_decision_with(&st, Some("/x/w"), Some(wc::ENGINE_DIR)) {
        wc::EnsureAction::Launch { program, args, .. } => {
            assert_eq!(program, "/x/w");
            assert_eq!(args, wc::engine_launch_args(Some(wc::ENGINE_DIR)));
        }
        other => panic!("ожидался запуск: {other:?}"),
    }
}

/// Юнит службы видит плагины пакета; освежение добавляет и снимает флаг
/// вслед за пакетом, не трогая выбранный человеком бинарь.
#[test]
fn autostart_unit_with_packaged_plugins() {
    let bin = "/home/tester/.local/share/waywallen/waywallen.AppImage";
    let with = wc::autostart_unit_content_with(bin, Some(wc::ENGINE_DIR));
    assert!(
        with.contains(&format!(
            "ExecStart=\"{bin}\" --no-ui --no-tray --plugin \"/usr/lib/rexon/engine\"\n"
        )),
        "{with}"
    );
    assert!(with.contains("KillMode=mixed"));
    assert_eq!(wc::unit_binary(&with).as_deref(), Some(bin));
    // Без пакета юнит байт в байт прежний (эталон Python).
    assert_eq!(wc::autostart_unit_content_with(bin, None), wc::autostart_unit_content(bin));
    assert!(wc::autostart_desktop_content_with(bin, Some(wc::ENGINE_DIR))
        .contains("--no-tray --plugin \"/usr/lib/rexon/engine\"\n"));

    let plain = wc::autostart_unit_content(bin);
    match wc::refresh_unit_plan_with(Some(&plain), None, HOME, Some(wc::ENGINE_DIR)) {
        wc::UnitRefresh::Rewrite { unit_content, .. } => assert_eq!(unit_content, with),
        other => panic!("пакет поставлен — юнит обязан получить --plugin: {other:?}"),
    }
    match wc::refresh_unit_plan_with(Some(&with), None, HOME, None) {
        wc::UnitRefresh::Rewrite { unit_content, .. } => assert_eq!(unit_content, plain),
        other => panic!("пакет снят — флаг обязан уйти: {other:?}"),
    }
    assert_eq!(
        wc::refresh_unit_plan_with(Some(&with), None, HOME, Some(wc::ENGINE_DIR)),
        wc::UnitRefresh::UpToDate
    );
    match wc::enable_plan_with(Some(bin), None, HOME, true, Some(wc::ENGINE_DIR)) {
        wc::EnablePlan::Systemd { unit_content, .. } => assert_eq!(unit_content, with),
        other => panic!("{other:?}"),
    }
}

/// 19.10.2026: `systemctl --user restart rexon-wallpaper-engine.service`
/// сразу выходил с кодом 0 — «waywallen: already running, no UI to
/// launch»: демон уже жил вне службы (поднят окном waywallen). Служба
/// стояла inactive как после успеха. Теперь условие запуска
/// (`ExecCondition`) спрашивает шину ДО старта: имя демона занято —
/// служба честно пропущена с причиной в журнале, а не «успешно» вышла.
#[test]
fn service_refuses_to_start_beside_a_foreign_daemon() {
    let unit = wc::autostart_unit_content_with("/x/waywallen.AppImage", Some(wc::ENGINE_DIR));
    let cond = unit
        .lines()
        .find_map(|l| l.strip_prefix("ExecCondition="))
        .expect("у службы обязано быть условие запуска");
    assert!(cond.contains(wc::DBUS_SERVICE), "{cond}");
    // systemd подставляет `$ПЕРЕМЕННЫЕ` и `%спецификаторы` в командах юнита.
    assert!(!cond.contains('$') && !cond.contains('%'), "{cond}");

    // Исполняем условие по-настоящему, с подставным busctl: свой каталог,
    // никакой шины и служб владельца.
    let dir = std::env::temp_dir().join(format!("rexon-execcond-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let busctl = dir.join("busctl");
    let argv: Vec<String> = {
        let cmd = cond.strip_prefix("/bin/sh -c ").expect("условие — /bin/sh -c «…»");
        let inner = cmd.strip_prefix('"').and_then(|c| c.strip_suffix('"')).expect("в кавычках");
        vec!["-c".into(), inner.to_string()]
    };
    let run = |owned: bool| {
        std::fs::write(&busctl, format!("#!/bin/sh\nexit {}\n", if owned { 0 } else { 1 })).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&busctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::process::Command::new("/bin/sh")
            .args(&argv)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .output()
            .unwrap()
    };
    let busy = run(true);
    let code = busy.status.code().unwrap();
    assert!((1..=254).contains(&code), "имя занято — пропуск (1..254), а не сбой: {code}");
    assert!(String::from_utf8_lossy(&busy.stderr).contains("already running"));
    assert_eq!(run(false).status.code(), Some(0), "демона нет — служба стартует");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Демон один и под службой: включён автозапуск (юнит записан) — вкладка
/// поднимает демона через службу, а не вторым отдельным процессом мимо неё.
#[test]
fn daemon_is_started_through_the_service_when_it_exists() {
    let dead = wc::DaemonStatus::default();
    match wc::ensure_running_decision_service(&dead, Some("/x/w"), Some(wc::ENGINE_DIR), true) {
        wc::EnsureAction::Launch { program, args, .. } => {
            let mut start = wc::autostart_unit_start_argv();
            assert_eq!(program, start.remove(0));
            assert_eq!(args, start);
            assert!(args.iter().any(|a| a.contains(&format!("systemctl --user start {}", wc::AUTOSTART_UNIT))));
        }
        other => panic!("ожидался запуск службы: {other:?}"),
    }
    // Службы нет — прежний отдельный запуск.
    match wc::ensure_running_decision_service(&dead, Some("/x/w"), None, false) {
        wc::EnsureAction::Launch { program, .. } => assert_eq!(program, "/x/w"),
        other => panic!("{other:?}"),
    }
    // Демон уже жив (чей угодно) — не трогаем.
    let live = wc::DaemonStatus { running: true, ws_port: 4242, ..Default::default() };
    assert!(matches!(
        wc::ensure_running_decision_service(&live, Some("/x/w"), None, true),
        wc::EnsureAction::Ready { .. }
    ));
}

/// Какая копия плагина работает, если он и в пакете, и у человека.
/// Решает демон (новее по версии, при равенстве — копия человека); здесь
/// сверяется наше зеркало этого правила.
#[test]
fn plugin_copy_choice_by_version() {
    use wc::PluginSource::{Bundled, User};
    assert_eq!(wc::active_plugin_source("0.3.0", "0.2.9"), Bundled, "в пакете новее");
    assert_eq!(wc::active_plugin_source("0.3.0", "0.3.1"), User, "у человека новее");
    assert_eq!(wc::active_plugin_source("0.3.0", "0.3.0"), User, "равные — копия человека");
    assert_eq!(wc::active_plugin_source("0.10.0", "0.9.9"), Bundled, "числами, не строкой");
    assert_eq!(wc::active_plugin_source("v0.3.0", "0.3.0"), User, "ведущая v не мешает");
}

fn copy(id: &str, version: &str) -> wc::PluginCopy {
    wc::PluginCopy { id: id.into(), name: String::new(), version: version.into() }
}

const OWE: &str = "org.waywallen.open-wallpaper-engine";

/// `plugin.toml` пакета: id, имя и версия; без id или версии — не плагин.
#[test]
fn plugin_manifest_copy_reads_identity() {
    let text = "[plugin]\nid = \"org.waywallen.open-wallpaper-engine\"\n\
                name = \"Wallpaper Engine\"\nversion = \"0.3.1\"\n";
    assert_eq!(
        wc::plugin_manifest_copy(text),
        Some(wc::PluginCopy {
            id: OWE.into(),
            name: "Wallpaper Engine".into(),
            version: "0.3.1".into()
        })
    );
    assert_eq!(wc::plugin_manifest_copy("[plugin]\nid = \"x\"\n"), None, "без версии");
    assert_eq!(wc::plugin_manifest_copy("[plugin]\nversion = \"1.0.0\"\n"), None, "без id");
    assert_eq!(wc::plugin_manifest_copy("не toml ["), None);
}

/// Активные копии из ответа `plugin_list`; ошибка демона или чужой ответ —
/// `None`, а не «плагинов нет» (иначе Rexon перезапускал бы демона зря).
#[test]
fn active_plugins_from_plugin_list_frame() {
    let frame = |status: pb::Status, payload| {
        pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(pb::Response {
                request_id: 7,
                status: status as i32,
                payload,
                ..Default::default()
            })),
        }
        .encode_to_vec()
    };
    let list = pb::response::Payload::PluginList(pb::PluginListResponse {
        plugins: vec![pb::PluginInfo {
            id: OWE.into(),
            name: "Wallpaper Engine".into(),
            version: "0.3.0".into(),
            system: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    let got = wc::active_plugins_from_frame(&frame(pb::Status::Ok, Some(list.clone())))
        .expect("ответ plugin_list");
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].id.as_str(), got[0].version.as_str()), (OWE, "0.3.0"));
    assert_eq!(wc::active_plugins_from_frame(&frame(pb::Status::Internal, Some(list))), None);
    assert_eq!(wc::active_plugins_from_frame(&frame(pb::Status::Ok, None)), None);
    assert_eq!(wc::active_plugins_from_frame(b"\xff\xff"), None);
}

/// Дефект 24.10.2026: пакет положил 0.3.1, а живой демон (запущен до
/// обновления пакета) держал копию человека 0.3.0 → перезапуск. Активная
/// новее или равна — так и задумано, демона не трогаем.
#[test]
fn stale_bundled_plugin_needs_restart() {
    let bundled = [copy(OWE, "0.3.1")];
    match wc::plugin_refresh_decision(&bundled, &[copy(OWE, "0.3.0")], None) {
        wc::PluginRefresh::Restart { key, stale } => {
            assert_eq!(stale.len(), 1);
            assert_eq!(stale[0].bundled_version, "0.3.1");
            assert_eq!(stale[0].active_version, "0.3.0");
            assert_eq!(stale[0].name, OWE, "без имени в манифесте — id");
            assert_eq!(key, wc::stale_plugins_key(&stale));
        }
        other => panic!("ждали перезапуск, получили {other:?}"),
    }
    assert_eq!(
        wc::plugin_refresh_decision(&bundled, &[copy(OWE, "0.3.1")], None),
        wc::PluginRefresh::Fine,
        "равные — копия человека законно"
    );
    assert_eq!(
        wc::plugin_refresh_decision(&bundled, &[copy(OWE, "0.4.0")], None),
        wc::PluginRefresh::Fine,
        "человек поставил новее пакета"
    );
    assert!(matches!(
        wc::plugin_refresh_decision(&[copy(OWE, "0.10.0")], &[copy(OWE, "0.9.9")], None),
        wc::PluginRefresh::Restart { .. }
    ), "версии числами, не строкой");
    assert_eq!(wc::plugin_refresh_decision(&[], &[copy(OWE, "0.1.0")], None), wc::PluginRefresh::Fine);
}

/// Демон без `--plugin` и без копии человека плагина не видит вовсе —
/// тоже перезапуск (наш запуск передаст флаг).
#[test]
fn missing_bundled_plugin_needs_restart() {
    match wc::plugin_refresh_decision(&[copy(OWE, "0.3.1")], &[copy("org.waywallen.image", "0.5.0")], None) {
        wc::PluginRefresh::Restart { stale, .. } => assert_eq!(stale[0].active_version, ""),
        other => panic!("ждали перезапуск, получили {other:?}"),
    }
}

/// То же расхождение второй раз — перезапуск не помог, по кругу не гоняем.
/// Другое расхождение (пакет снова обновился) — новая попытка.
#[test]
fn plugin_restart_is_not_repeated() {
    let bundled = [copy(OWE, "0.3.1")];
    let active = [copy(OWE, "0.3.0")];
    let wc::PluginRefresh::Restart { key, .. } = wc::plugin_refresh_decision(&bundled, &active, None)
    else {
        panic!("первый раз — перезапуск");
    };
    assert!(matches!(
        wc::plugin_refresh_decision(&bundled, &active, Some(&key)),
        wc::PluginRefresh::GiveUp { .. }
    ));
    assert!(matches!(
        wc::plugin_refresh_decision(&[copy(OWE, "0.3.2")], &active, Some(&key)),
        wc::PluginRefresh::Restart { .. }
    ));
}

/// Занятый демон (идёт задача) не перезапускаем.
#[test]
fn busy_daemon_is_not_restarted() {
    let task = |state: &str| wc::DaemonTask {
        id: 1,
        kind: "download".into(),
        name: "x".into(),
        started_at_ms: 0,
        state: state.into(),
    };
    assert!(wc::daemon_busy(&[task("completed"), task("running")]));
    assert!(!wc::daemon_busy(&[task("completed"), task("failed: x"), task("cancelled")]));
    assert!(!wc::daemon_busy(&[]));
}

/// Ключ попытки в журнале читается «у демона → в пакете», а не
/// «0.3.2<0.3.1» (так выходило «0.3.2 меньше 0.3.1», 30.10.2026).
#[test]
fn refresh_keys_read_from_daemon_to_package() {
    let stale = |active: &str| wc::StalePlugin {
        id: OWE.into(),
        name: "Wallpaper Engine".into(),
        bundled_version: "0.3.2".into(),
        active_version: active.into(),
    };
    assert_eq!(wc::stale_plugins_key(&[stale("0.3.1")]), format!("{OWE}=0.3.1→0.3.2"));
    assert_eq!(wc::stale_plugins_key(&[stale("")]), format!("{OWE}=нет→0.3.2"));
    assert_eq!(wc::engine_refresh_key("0.5.0", "0.5.1"), "engine=0.5.0→0.5.1");
}

/// Службы и ждущий сторож дисплея демона 0.5.0 «занятым» его не делают:
/// иначе на столе без вывода перезапуск откладывался вечно (MATE, 30.10.2026).
/// Строка — живой ответ `busctl … ListTasks` того демона, сокращённый.
#[test]
fn idle_daemon_with_services_is_not_busy() {
    let live = r#"{"type":"a(tssxs)","data":[[[1,"service","service/audio",1793316441834,"running"],[9,"service","display/endpoint",1793316441834,"running"],[11,"startup","boot/display-watcher",1793316441834,"running"],[12,"startup","startup/restore",1793316441834,"completed"],[21,"generic","scan/refresh",1793398079083,"completed"]]]}"#;
    let tasks = wc::parse_busctl_tasks(live);
    assert_eq!(tasks.len(), 5);
    assert!(!wc::daemon_busy(&tasks), "только службы и ждущий сторож — демон свободен");

    let mut scanning = tasks.clone();
    scanning.push(wc::DaemonTask {
        id: 30,
        kind: "generic".into(),
        name: "scan/refresh".into(),
        started_at_ms: 0,
        state: "running".into(),
    });
    assert!(wc::daemon_busy(&scanning), "идёт сканирование — занят");

    let mut applying = tasks;
    applying.push(wc::DaemonTask {
        id: 31,
        kind: "apply".into(),
        name: "apply/8496".into(),
        started_at_ms: 0,
        state: "running".into(),
    });
    assert!(wc::daemon_busy(&applying), "идёт применение — занят");
}

/// Текст уведомления называет плагин и обе версии.
#[test]
fn plugin_refresh_notice_names_versions() {
    let _lang = i18n_pin::en();
    let stale = vec![
        wc::StalePlugin {
            id: OWE.into(),
            name: "Wallpaper Engine".into(),
            bundled_version: "0.3.1".into(),
            active_version: "0.3.0".into(),
        },
        wc::StalePlugin {
            id: "b".into(),
            name: "B".into(),
            bundled_version: "1.0.0".into(),
            active_version: String::new(),
        },
    ];
    let body = wc::plugin_refresh_body(&stale);
    assert!(body.contains("Wallpaper Engine 0.3.1 instead of 0.3.0"), "{body}");
    assert!(body.contains("B 1.0.0 (was not loaded)"), "{body}");
    assert!(!wc::plugin_refresh_title().is_empty());
}

/// Сторож пакета следит за родителем каталога движка, за ним самим (только
/// `plugins`), за `plugins/` целиком и за `plugin.toml` каждого плагина.
#[test]
fn engine_watch_targets_cover_the_package_layout() {
    let t = wc::engine_watch_targets(
        &wc::EngineLayout::bundled_in("/usr/lib/rexon/engine/"),
        &[OWE.to_string(), String::new(), "a/b".into()],
    );
    let get = |d: &str| t.iter().find(|x| x.dir == d).map(|x| x.names.clone());
    assert_eq!(get("/usr/lib/rexon"), Some(vec!["engine".to_string()]));
    assert_eq!(
        get("/usr/lib/rexon/engine"),
        Some(vec!["plugins".to_string(), wc::ENGINE_APPIMAGE_NAME.to_string()])
    );
    assert_eq!(get("/usr/lib/rexon/engine/plugins"), Some(vec![]));
    assert_eq!(
        get(&format!("/usr/lib/rexon/engine/plugins/{OWE}")),
        Some(vec!["plugin.toml".to_string()])
    );
    assert_eq!(t.len(), 4, "пустые и составные имена папок не берутся: {t:?}");
    assert!(wc::ENGINE_WATCH_QUIET_MS >= 5000 && wc::ENGINE_WATCH_QUIET_MS <= 10_000);
}

/// Живой inotify на раскладке пакета во временном каталоге: новый
/// `plugin.toml` (и его замена через `rename`, как у пакетного менеджера) и
/// новая папка плагина будят сторожа, и замена AppImage движка — тоже;
/// запись библиотеки плагина — нет.
#[test]
fn engine_watch_wakes_on_plugin_manifest_only() {
    use rexon_core::fs_watch::Watch;
    let root = std::env::temp_dir().join(format!("rexon-engine-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let engine = root.join("engine");
    let owe = engine.join("plugins").join(OWE);
    std::fs::create_dir_all(&owe).unwrap();
    std::fs::write(owe.join("plugin.toml"), "[plugin]\nid='x'\nversion='0.3.0'\n").unwrap();
    let layout = wc::EngineLayout::bundled_in(&engine.to_string_lossy());
    let mut w = Watch::new(wc::engine_watch_targets(&layout, &[OWE.to_string()])).expect("inotify");

    std::fs::write(owe.join("libowe.so"), "x").unwrap();
    std::fs::write(engine.join("other.txt"), "x").unwrap();
    assert!(!w.wait(200), "библиотека плагина и посторонний файл — не повод");

    let img_tmp = engine.join("waywallen.AppImage.tmp");
    std::fs::write(&img_tmp, "new").unwrap();
    std::fs::rename(&img_tmp, engine.join(wc::ENGINE_APPIMAGE_NAME)).unwrap();
    assert!(w.wait(500), "замена AppImage движка");
    while w.wait(100) {}

    let tmp = owe.join("plugin.toml.tmp");
    std::fs::write(&tmp, "[plugin]\nid='x'\nversion='0.3.1'\n").unwrap();
    std::fs::rename(&tmp, owe.join("plugin.toml")).unwrap();
    assert!(w.wait(500), "замена plugin.toml");
    while w.wait(100) {}

    std::fs::create_dir_all(engine.join("plugins").join("org.example.new")).unwrap();
    assert!(w.wait(500), "новая папка плагина");
    let _ = std::fs::remove_dir_all(&root);
}

/// Движок пакетов дистрибутива: демон в `/usr/bin` вне каталога данных —
/// за ним отдельная цель (только его имя), каталог данных — только `plugins`.
#[test]
fn engine_watch_targets_cover_the_system_layout() {
    let layout = wc::EngineLayout::system_in(wc::SYSTEM_DAEMON, wc::SYSTEM_ENGINE_DIR);
    let t = wc::engine_watch_targets(&layout, &[OWE.to_string()]);
    let get = |d: &str| t.iter().find(|x| x.dir == d).map(|x| x.names.clone());
    assert_eq!(get("/usr/share"), Some(vec!["waywallen".to_string()]));
    assert_eq!(get("/usr/bin"), Some(vec!["waywallen".to_string()]));
    assert_eq!(get("/usr/share/waywallen"), Some(vec!["plugins".to_string()]));
    assert_eq!(get("/usr/share/waywallen/plugins"), Some(vec![]));
    assert_eq!(
        get(&format!("/usr/share/waywallen/plugins/{OWE}")),
        Some(vec!["plugin.toml".to_string()])
    );
    assert_eq!(t.len(), 5, "{t:?}");
}

/// Раскладки движка пакетов: пакеты дистрибутива первыми, вшитый комплект —
/// запасным; пути — те, куда их ставят пакеты Arch (`waywallen`,
/// `open-wallpaper-engine`).
#[test]
fn engine_layouts_order_and_paths() {
    let l = wc::engine_layouts();
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].kind, wc::EngineKind::System);
    assert_eq!(l[0].daemon, "/usr/bin/waywallen");
    assert_eq!(l[0].plugins_dir(), "/usr/share/waywallen/plugins");
    assert_eq!(l[1].kind, wc::EngineKind::Bundled);
    assert_eq!(l[1].daemon, "/usr/lib/rexon/engine/waywallen.AppImage");
    assert_eq!(l[1].plugins_dir(), "/usr/lib/rexon/engine/plugins");
    assert_eq!(wc::layout_of(&l, "/usr/bin/waywallen").map(|x| x.kind), Some(wc::EngineKind::System));
    assert_eq!(wc::layout_of(&l, "/home/u/.local/share/waywallen/waywallen.AppImage"), None);
}

/// `--plugin` уходит демону, только если он не видит каталог пакета сам:
/// `waywallen` из пакета живёт в `/usr/bin` и смотрит в
/// `/usr/share/waywallen/plugins` без флага (повтор флагом дал бы двойные
/// записи), а `waywallen-bin` (`/opt/…/AppRun`) и AppImage человека — нет.
#[test]
fn plugin_root_only_when_daemon_does_not_see_it() {
    let system = wc::EngineLayout::system_in(wc::SYSTEM_DAEMON, wc::SYSTEM_ENGINE_DIR);
    let bundled = wc::EngineLayout::bundled_in(wc::ENGINE_DIR);
    assert_eq!(wc::daemon_own_engine_dir("/usr/bin/waywallen").as_deref(), Some("/usr/share/waywallen"));
    assert_eq!(wc::plugin_root_for(&[system.clone()], Some("/usr/bin/waywallen")), None);
    assert_eq!(
        wc::plugin_root_for(&[system.clone()], Some("/opt/waywallen-bin/AppRun")).as_deref(),
        Some("/usr/share/waywallen")
    );
    let managed = wc::managed_appimage_path(HOME);
    assert_eq!(
        wc::plugin_root_for(&[system.clone()], Some(&managed)).as_deref(),
        Some("/usr/share/waywallen")
    );
    // Вшитый комплект флагом получает любой демон.
    assert_eq!(
        wc::plugin_root_for(&[system.clone(), bundled.clone()], Some("/usr/bin/waywallen")).as_deref(),
        Some(wc::ENGINE_DIR)
    );
    assert_eq!(wc::plugin_root_for(&[], Some("/usr/bin/waywallen")), None);
    // Демон неизвестен — флаг нужен (лишним он станет только у демона из /usr/bin).
    assert_eq!(wc::plugin_root_for(&[system], None).as_deref(), Some("/usr/share/waywallen"));
}

/// Демон пакета дистрибутива стоит перед вшитым комплектом (стоят оба —
/// запускается системный) и перед управляемым AppImage; правило
/// «управляемый — только если строго новее» одно для обеих раскладок.
#[test]
fn system_engine_before_bundled_and_managed() {
    const ENGINE: &str = "/opt/test-engine";
    let packaged = wc::engine_appimage_path(ENGINE);
    let system = wc::SYSTEM_DAEMON.to_string();
    let managed = wc::managed_appimage_path(HOME);
    let cands: Vec<String> = wc::daemon_binary_candidates_in(PKG, HOME, ENGINE)
        .into_iter()
        .map(|c| match c {
            wc::DaemonCandidate::File(p) | wc::DaemonCandidate::PathLookup(p) => p,
        })
        .collect();
    let at = |p: &str| cands.iter().position(|c| c == p).unwrap();
    assert!(at(&system) < at(&packaged) && at(&packaged) < at(&managed), "{cands:?}");

    // Стоят и системный, и вшитый — системный.
    let mut fs = VersionedFs { exec_files: vec![packaged.clone(), system.clone()], versions: HashMap::new(), asked: vec![] };
    assert_eq!(wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE), Some(system.clone()));

    // Только системный — он, версии не спрашиваем.
    let mut fs = VersionedFs { exec_files: vec![system.clone()], versions: HashMap::new(), asked: vec![] };
    assert_eq!(wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE), Some(system.clone()));
    assert!(fs.asked.is_empty());

    let pick = |s: &str, m: &str| {
        let versions = HashMap::from([(system.clone(), s.to_string()), (managed.clone(), m.to_string())]);
        let mut fs = VersionedFs { exec_files: vec![system.clone(), managed.clone()], versions, asked: vec![] };
        wc::find_daemon_binary_in(&mut fs, PKG, HOME, ENGINE).unwrap()
    };
    assert_eq!(pick("0.5.0", "0.5.1"), managed, "управляемый новее — он уже переписал базу");
    assert_eq!(pick("0.5.0", "0.5.0"), system, "равные — пакет системы");
    assert_eq!(pick("0.5.1", "0.5.0"), system, "система новее");
}

/// Юнит, записанный на пропавший движок (пакет перестал вшивать AppImage),
/// пересобирается на найденном; живой выбор и «ничего не нашлось» — прежний.
#[test]
fn unit_refresh_follows_vanished_engine() {
    let old = wc::engine_appimage_path(wc::ENGINE_DIR);
    assert_eq!(wc::unit_refresh_binary(&old, false, Some(wc::SYSTEM_DAEMON)), wc::SYSTEM_DAEMON);
    assert_eq!(wc::unit_refresh_binary(&old, true, Some(wc::SYSTEM_DAEMON)), old);
    assert_eq!(wc::unit_refresh_binary(&old, false, None), old);

    let written = wc::autostart_unit_content_with(&old, Some(wc::ENGINE_DIR));
    match wc::refresh_unit_plan_on(Some(&written), wc::SYSTEM_DAEMON, None, HOME, None) {
        wc::UnitRefresh::Rewrite { unit_content, .. } => {
            assert_eq!(wc::unit_binary(&unit_content).as_deref(), Some(wc::SYSTEM_DAEMON));
            assert!(!unit_content.contains(wc::PLUGIN_FLAG), "демону из /usr/bin флаг не нужен");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(wc::refresh_unit_plan_on(None, wc::SYSTEM_DAEMON, None, HOME, None), wc::UnitRefresh::Skip);
}

/// Уведомления о перезапуске называют, кто принёс новое: обновление Rexon
/// (вшитый комплект) или обновление системы (пакеты дистрибутива).
#[test]
fn refresh_texts_name_who_brought_the_update() {
    // Язык — общий на процесс: без закрепления соседний тест переключает
    // его посреди сравнения.
    let _lang = i18n_pin::en();
    let b = wc::engine_refresh_body_for(wc::EngineKind::Bundled, "0.5.0", "waywallen 0.5.1");
    let s = wc::engine_refresh_body_for(wc::EngineKind::System, "0.5.0", "waywallen 0.5.1");
    assert_eq!(b, wc::engine_refresh_body("0.5.0", "waywallen 0.5.1"));
    assert_ne!(b, s);
    assert!(s.contains("0.5.0") && s.contains("0.5.1"), "{s}");
    let stale = [wc::StalePlugin {
        id: OWE.into(),
        name: "OWE".into(),
        bundled_version: "0.3.3".into(),
        active_version: "0.3.2".into(),
    }];
    assert_eq!(wc::plugin_refresh_body(&stale), wc::plugin_refresh_body_for(wc::EngineKind::Bundled, &stale));
    assert_ne!(
        wc::plugin_refresh_body_for(wc::EngineKind::Bundled, &stale),
        wc::plugin_refresh_body_for(wc::EngineKind::System, &stale)
    );
}

// ============================================================================
// 2. Автозапуск
// ============================================================================

#[test]
fn содержимое_автозапуска() {
    let g = golden();
    let a = &g["autostart"];
    let binary = format!("{HOME}/.local/share/waywallen/waywallen.AppImage");

    assert_eq!(a["unit_name"].as_str().unwrap(), wc::AUTOSTART_UNIT);
    assert_eq!(a["desktop_name"].as_str().unwrap(), wc::AUTOSTART_DESKTOP);

    // ⚠️ Намеренное расхождение (починка бага #3 шапки модуля): к эталону
    // Python добавлены `KillMode=mixed` и объясняющий его комментарий, а
    // 19.10.2026 — `ExecCondition` (служба не стартует рядом с чужим демоном).
    // Без них systemd на каждой остановке валил демона сигналом BUS —
    // AppImage размонтировался под ещё работающим процессом. Всё
    // ОСТАЛЬНОЕ содержимое обязано совпадать с Python байт в байт.
    let unit = wc::autostart_unit_content(&binary);
    let без_починки: String = unit
        .lines()
        .filter(|l| {
            !l.starts_with("KillMode=") && !l.starts_with("# Демон живёт в AppImage")
                && !l.starts_with("# cgroup. SIGTERM только главному")
                && !l.starts_with("# демоном и завершение падает")
                && !l.starts_with("ExecCondition=")
                && !l.starts_with("# Демон уже жив вне службы")
                && !l.starts_with("# копии не будет: служба честно")
                && !l.starts_with("# выходит «успешно» со словами")
        })
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(без_починки, a["unit_content"].as_str().unwrap());
    // Переснято 01.10.2026: `Comment=` английский + `Comment[<код>]` из
    // каталога (прежде был только `Comment[ru]`), `Name[ru]` — из каталога.
    assert_eq!(
        i18n_pin::ru_only(&wc::autostart_desktop_content(&binary)),
        a["desktop_content"].as_str().unwrap()
    );
}

/// Правило движка (#12), а не заплатка в одном юните: если systemd-служба
/// запускает AppImage, её бинарь исполняется из FUSE-монтирования, которое
/// держит соседний процесс в той же cgroup. Умолчательный
/// `KillMode=control-group` шлёт SIGTERM обоим разом — образ отваливается под
/// ещё живым процессом, и штатное завершение оканчивается SIGBUS. Проверяем
/// ВСЕ шаблоны юнитов, которые Rexon пишет пользователю.
#[test]
fn юниты_с_appimage_обязаны_нести_killmode_mixed() {
    let проверить = |имя: &str, unit: &str| {
        let из_appimage = unit
            .lines()
            .filter(|l| l.starts_with("ExecStart="))
            .any(|l| l.to_ascii_lowercase().contains(".appimage"));
        if !из_appimage {
            return;
        }
        assert!(
            unit.lines().any(|l| l.trim() == "KillMode=mixed"),
            "{имя}: ExecStart ведёт на AppImage, но KillMode=mixed нет — \
             остановка службы даст SIGBUS (разбор в шапке we_client)"
        );
    };

    проверить(
        wc::AUTOSTART_UNIT,
        &wc::autostart_unit_content(&format!(
            "{HOME}/.local/share/waywallen/waywallen.AppImage"
        )),
    );
    проверить(
        "rexon-session-splash.service",
        &rexon_core::session_splash::systemd_unit_content("/usr/bin/rexon --session-splash"),
    );
}

/// Освежение устаревшего юнита: путь движка берётся из СТАРОГО `ExecStart`,
/// правится только обвязка. Иначе починки шаблона доставались бы лишь тем, кто
/// заново щёлкнул «Start at login».
#[test]
fn освежение_устаревшего_юнита() {
    let binary = format!("{HOME}/.local/share/waywallen/waywallen.AppImage");
    let свежий = wc::autostart_unit_content(&binary);

    // Файла нет — трогать нечего.
    assert_eq!(wc::refresh_unit_plan(None, None, HOME), wc::UnitRefresh::Skip);
    // Мусор без ExecStart — не переписываем вслепую.
    assert_eq!(
        wc::refresh_unit_plan(Some("[Unit]\n"), None, HOME),
        wc::UnitRefresh::Skip
    );
    // Актуальный юнит не трогаем.
    assert_eq!(
        wc::refresh_unit_plan(Some(&свежий), None, HOME),
        wc::UnitRefresh::UpToDate
    );

    // Юнит прежнего образца (без KillMode) — переписать, сохранив бинарь.
    let старый: String = свежий
        .lines()
        .filter(|l| !l.starts_with("KillMode=") && !l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect();
    match wc::refresh_unit_plan(Some(&старый), None, HOME) {
        wc::UnitRefresh::Rewrite {
            unit_path,
            unit_content,
            daemon_reload_argv,
        } => {
            assert_eq!(unit_path, wc::autostart_unit_path(None, HOME));
            assert_eq!(unit_content, свежий);
            assert_eq!(daemon_reload_argv, wc::autostart_daemon_reload_argv());
        }
        other => panic!("ожидался Rewrite, получено {other:?}"),
    }

    // Путь движка выбрал пользователь — подменять его нельзя.
    let свой = "/opt/waywallen/waywallen.AppImage";
    let чужой: String = wc::autostart_unit_content(свой)
        .lines()
        .filter(|l| !l.starts_with("KillMode="))
        .map(|l| format!("{l}\n"))
        .collect();
    match wc::refresh_unit_plan(Some(&чужой), None, HOME) {
        wc::UnitRefresh::Rewrite { unit_content, .. } => {
            assert_eq!(unit_content, wc::autostart_unit_content(свой));
        }
        other => panic!("ожидался Rewrite, получено {other:?}"),
    }
}

#[test]
fn бинарь_из_записанного_юнита() {
    let binary = format!("{HOME}/.local/share/waywallen/waywallen.AppImage");
    assert_eq!(
        wc::unit_binary(&wc::autostart_unit_content(&binary)).as_deref(),
        Some(binary.as_str())
    );
    // Без кавычек (юнит, поправленный руками) и с пробелами по краям.
    assert_eq!(
        wc::unit_binary("[Service]\n  ExecStart=/usr/bin/waywallen --no-ui\n").as_deref(),
        Some("/usr/bin/waywallen")
    );
    assert_eq!(wc::unit_binary("[Service]\nType=simple\n"), None);
    assert_eq!(wc::unit_binary("ExecStart=\n"), None);
}

#[test]
fn пути_автозапуска() {
    let g = golden();
    let a = &g["autostart"];

    assert_eq!(
        wc::autostart_unit_path(None, HOME),
        a["unit_path_default"].as_str().unwrap()
    );
    assert_eq!(
        wc::autostart_desktop_path(None, HOME),
        a["desktop_path_default"].as_str().unwrap()
    );
    assert_eq!(
        wc::autostart_desktop_path(Some("/tmp/cfg"), HOME),
        a["desktop_path_xdg"].as_str().unwrap()
    );

    // ⚠️ Намеренное расхождение (починка бага #2 шапки модуля): Python при
    // заданном XDG_CONFIG_HOME всё равно писал юнит в ~/.config/systemd/user,
    // где systemd его не ищет → `systemctl --user enable` падал.
    let py_quirk = a["unit_path_xdg_python_quirk"].as_str().unwrap();
    assert_eq!(py_quirk, format!("{HOME}/.config/systemd/user/{}", wc::AUTOSTART_UNIT));
    assert_eq!(
        wc::autostart_unit_path(Some("/tmp/cfg"), HOME),
        format!("/tmp/cfg/systemd/user/{}", wc::AUTOSTART_UNIT)
    );
}

#[test]
fn argv_systemctl() {
    assert_eq!(
        wc::autostart_is_enabled_argv(),
        vec!["systemctl", "--user", "is-enabled", wc::AUTOSTART_UNIT]
    );
    assert_eq!(
        wc::autostart_enable_argv(),
        vec!["systemctl", "--user", "enable", wc::AUTOSTART_UNIT]
    );
    assert_eq!(
        wc::autostart_disable_argv(),
        vec!["systemctl", "--user", "disable", wc::AUTOSTART_UNIT]
    );
    assert_eq!(
        wc::autostart_daemon_reload_argv(),
        vec!["systemctl", "--user", "daemon-reload"]
    );
    assert_eq!(wc::systemd_runtime_dir(1000), "/run/user/1000/systemd");
    assert!(wc::systemd_user_available(true, true));
    assert!(!wc::systemd_user_available(true, false));
    assert!(!wc::systemd_user_available(false, true));
}

#[test]
fn вердикт_autostart_enabled() {
    // systemd сказал enabled — этого достаточно.
    assert!(wc::autostart_enabled(Some((0, "enabled\n")), false));
    // ненулевой код / другой статус → смотрим XDG.
    assert!(!wc::autostart_enabled(Some((1, "disabled\n")), false));
    assert!(wc::autostart_enabled(Some((1, "disabled\n")), true));
    assert!(!wc::autostart_enabled(Some((0, "static")), false));
    // systemd недоступен / вызов упал → только XDG.
    assert!(wc::autostart_enabled(None, true));
    assert!(!wc::autostart_enabled(None, false));
}

#[test]
fn планы_автозапуска() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let msgs = &g["autostart"]["messages"];
    let binary = format!("{HOME}/.local/share/waywallen/waywallen.AppImage");

    // Выключение при живом systemd и существующем юните.
    let p = wc::disable_plan(None, HOME, true, true);
    assert_eq!(p.disable_argv, Some(wc::autostart_disable_argv()));
    assert_eq!(p.daemon_reload_argv, Some(wc::autostart_daemon_reload_argv()));
    assert_eq!(p.remove_unit, wc::autostart_unit_path(None, HOME));
    assert_eq!(p.remove_desktop, wc::autostart_desktop_path(None, HOME));
    assert_eq!(p.message, msgs["disabled"].as_str().unwrap());

    // Юнита нет — disable не зовём.
    let p = wc::disable_plan(None, HOME, true, false);
    assert_eq!(p.disable_argv, None);

    // Без systemd — ни disable, ни daemon-reload.
    let p = wc::disable_plan(None, HOME, false, true);
    assert_eq!(p.disable_argv, None);
    assert_eq!(p.daemon_reload_argv, None);

    // Включение без бинаря.
    match wc::enable_plan(None, None, HOME, true) {
        wc::EnablePlan::NoBinary { message } => {
            assert_eq!(message, msgs["no_binary"].as_str().unwrap());
        }
        other => panic!("ожидался NoBinary, получено {other:?}"),
    }

    // Включение через systemd.
    match wc::enable_plan(Some(&binary), None, HOME, true) {
        wc::EnablePlan::Systemd {
            unit_path,
            unit_content,
            daemon_reload_argv,
            enable_argv,
            remove_desktop,
            message,
        } => {
            assert_eq!(unit_path, wc::autostart_unit_path(None, HOME));
            // Само содержимое сверено с эталоном Python в
            // `содержимое_автозапуска` (там же — намеренное расхождение по
            // `KillMode`); здесь важно, что план несёт именно его.
            assert_eq!(unit_content, wc::autostart_unit_content(&binary));
            assert_eq!(daemon_reload_argv, wc::autostart_daemon_reload_argv());
            assert_eq!(enable_argv, wc::autostart_enable_argv());
            assert_eq!(remove_desktop, wc::autostart_desktop_path(None, HOME));
            assert_eq!(message, msgs["systemd"].as_str().unwrap());
        }
        other => panic!("ожидался Systemd, получено {other:?}"),
    }

    // Фолбэк XDG.
    match wc::enable_plan(Some(&binary), None, HOME, false) {
        wc::EnablePlan::Xdg { desktop_path, desktop_content, message } => {
            assert_eq!(desktop_path, wc::autostart_desktop_path(None, HOME));
            assert_eq!(
                i18n_pin::ru_only(&desktop_content),
                g["autostart"]["desktop_content"].as_str().unwrap()
            );
            assert_eq!(message, msgs["xdg"].as_str().unwrap());
        }
        other => panic!("ожидался Xdg, получено {other:?}"),
    }
}

#[test]
fn тексты_ошибок_автозапуска() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        wc::enable_failed_message("  Unit not found\n", "прочее"),
        "systemctl enable: Unit not found"
    );
    assert_eq!(
        wc::enable_failed_message("   ", "  из stdout "),
        "systemctl enable: из stdout"
    );
    assert_eq!(
        wc::set_autostart_error_message("[Errno 13] Permission denied"),
        "Не удалось настроить автозапуск: [Errno 13] Permission denied"
    );
}

// ============================================================================
// 3. Решения DaemonControl
// ============================================================================

/// Транспорт-заглушка: отдаёт заранее заданные значения свойств и журналирует
/// порядок обращений (демон НЕ дёргается).
struct FakeIo {
    props: HashMap<String, DbusValue>,
    asked: Vec<String>,
}

impl FakeIo {
    fn new(pairs: &[(&str, DbusValue)]) -> Self {
        FakeIo {
            props: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            asked: Vec::new(),
        }
    }
}

impl WaywallenIo for FakeIo {
    fn is_valid(&mut self) -> bool {
        true
    }
    fn get_property(&mut self, name: &str) -> DbusValue {
        self.asked.push(name.to_string());
        self.props.get(name).cloned().unwrap_or(DbusValue::None)
    }
    fn call(&mut self, _method: &str, _args: &[DbusValue]) -> DbusReply {
        DbusReply::default()
    }
}

#[test]
fn probe_читает_свойства_через_общий_трейт() {
    let mut io = FakeIo::new(&[
        ("WsPort", DbusValue::Int(37281)),
        ("Version", DbusValue::Str("0.2.5".into())),
        ("CurrentWallpaperId", DbusValue::Str("1234567890".into())),
    ]);
    let st = wc::probe(&mut io);
    assert_eq!(
        st,
        wc::DaemonStatus {
            running: true,
            version: "0.2.5".into(),
            ws_port: 37281,
            current_wallpaper_id: "1234567890".into(),
        }
    );
    assert_eq!(io.asked, vec!["WsPort", "Version", "CurrentWallpaperId"]);

    // Демон молчит: WsPort=None → остальные свойства не запрашиваются вообще.
    let mut io = FakeIo::new(&[]);
    assert_eq!(wc::probe(&mut io), wc::DaemonStatus::default());
    assert_eq!(io.asked, vec!["WsPort"]);

    // Пустые Version/CurrentWallpaperId → пустые строки (`str(x or "")`).
    let mut io = FakeIo::new(&[
        ("WsPort", DbusValue::Int(1)),
        ("Version", DbusValue::Str(String::new())),
    ]);
    let st = wc::probe(&mut io);
    assert_eq!(st.version, "");
    assert_eq!(st.current_wallpaper_id, "");
}

/// ⚠️ Починка бага оригинала: `int(ws_port)` на нечисловой строке падал
/// необработанным `ValueError` — вкладка WE не открывалась. Здесь 0.
#[test]
fn probe_не_падает_на_нечисловом_ws_port() {
    let mut io = FakeIo::new(&[("WsPort", DbusValue::Str("нет".into()))]);
    let st = wc::probe(&mut io);
    assert!(st.running);
    assert_eq!(st.ws_port, 0);
    // ws_port == 0 уводит ensure_running в ветку автозапуска.
    match wc::ensure_running_decision(&st, Some("/bin/w")) {
        wc::EnsureAction::Launch { .. } => {}
        other => panic!("ожидался Launch, получено {other:?}"),
    }
}

#[test]
fn решение_ensure_running() {
    let g = golden();
    let live = wc::DaemonStatus {
        running: true,
        ws_port: 37281,
        version: "0.2.5".into(),
        current_wallpaper_id: String::new(),
    };
    let action = wc::ensure_running_decision(&live, None);
    assert_eq!(action, wc::EnsureAction::Ready { status: live.clone() });
    assert!(action.ensure_kde_plugin());

    let dead = wc::DaemonStatus::default();
    let action = wc::ensure_running_decision(&dead, None);
    assert_eq!(
        action,
        wc::EnsureAction::NoBinary {
            message: g["messages"]["not_found"].as_str().unwrap().to_string()
        }
    );
    // Плагин ставим ТОЛЬКО при живом/запускаемом движке: иначе containment
    // подменит обои пользователя чёрным экраном.
    assert!(!action.ensure_kde_plugin());

    let binary = format!("{HOME}/.local/share/waywallen/waywallen.AppImage");
    let action = wc::ensure_running_decision(&dead, Some(&binary));
    assert_eq!(
        action,
        wc::EnsureAction::Launch {
            program: binary.clone(),
            args: vec!["--no-ui".into(), "--no-tray".into()],
            polls_left: g["dbus"]["poll_limit"].as_i64().unwrap(),
            poll_interval_ms: g["dbus"]["poll_ms"].as_u64().unwrap(),
        }
    );
    assert!(action.ensure_kde_plugin());

    assert_eq!(
        wc::daemon_launch_failed_message(&binary),
        g["messages"]["launch_failed"].as_str().unwrap()
    );
}

#[test]
fn решение_поллинга() {
    let g = golden();
    let live = wc::DaemonStatus { running: true, ws_port: 1, ..Default::default() };
    assert_eq!(
        wc::poll_decision(&live, 24),
        wc::PollAction::Ready { status: live.clone() }
    );

    let dead = wc::DaemonStatus::default();
    assert_eq!(
        wc::poll_decision(&dead, 24),
        wc::PollAction::Continue { polls_left: 23 }
    );
    assert_eq!(
        wc::poll_decision(&dead, 1),
        wc::PollAction::Timeout {
            message: g["messages"]["poll_timeout"].as_str().unwrap().to_string()
        }
    );
}

#[test]
fn адреса_dbus_единые_с_основным_клиентом() {
    let g = golden();
    assert_eq!(wc::DBUS_SERVICE, g["dbus"]["service"].as_str().unwrap());
    assert_eq!(wc::DBUS_PATH, g["dbus"]["path"].as_str().unwrap());
    assert_eq!(wc::DBUS_IFACE, g["dbus"]["iface"].as_str().unwrap());
    // ⚠️ Второго источника адресов нет — реэкспорт waywallen_client.
    assert_eq!(wc::DBUS_SERVICE, rexon_core::waywallen_client::BUS_NAME);
}

#[test]
fn разбор_list_tasks() {
    let g = golden();
    let argv: Vec<String> = g["busctl"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(wc::busctl_tasks_argv(), argv);

    for key in ["ok", "empty", "broken"] {
        let stdout = g["busctl"]["stdout"][key].as_str().unwrap();
        let expect: Vec<wc::DaemonTask> = g["busctl"]["parsed"][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| wc::DaemonTask {
                id: r[0].as_i64().unwrap(),
                kind: r[1].as_str().unwrap().to_string(),
                name: r[2].as_str().unwrap().to_string(),
                started_at_ms: r[3].as_i64().unwrap(),
                state: r[4].as_str().unwrap().to_string(),
            })
            .collect();
        assert_eq!(wc::parse_busctl_tasks(stdout), expect, "busctl {key}");
    }
}

#[test]
fn разбор_строк_qtdbus() {
    let rows = vec![vec![
        DbusValue::Int(1),
        DbusValue::Str("scan".into()),
        DbusValue::Str("Библиотека".into()),
        DbusValue::Int(1785000896000),
        DbusValue::Str("running".into()),
    ]];
    assert_eq!(
        wc::tasks_from_dbus_rows(&rows),
        Some(vec![wc::DaemonTask {
            id: 1,
            kind: "scan".into(),
            name: "Библиотека".into(),
            started_at_ms: 1785000896000,
            state: "running".into(),
        }])
    );
    assert_eq!(wc::tasks_from_dbus_rows(&[]), Some(vec![]));

    // Не 5 элементов / нечисловой id → фолбэк на busctl (в Python это
    // TypeError/ValueError в цикле).
    assert_eq!(wc::tasks_from_dbus_rows(&[vec![DbusValue::Int(1)]]), None);
    let bad = vec![vec![
        DbusValue::Str("х".into()),
        DbusValue::Str("s".into()),
        DbusValue::Str("n".into()),
        DbusValue::Int(0),
        DbusValue::Str("running".into()),
    ]];
    assert_eq!(wc::tasks_from_dbus_rows(&bad), None);
}

#[test]
fn быстрые_методы_и_остановка_обоев() {
    assert_eq!(wc::dbus_quick_result(&[]), "");
    assert_eq!(wc::dbus_quick_result(&[DbusValue::Int(7)]), "7");
    assert_eq!(wc::dbus_quick_result(&[DbusValue::Str("ok".into())]), "ok");

    let dead = wc::DaemonStatus::default();
    assert_eq!(wc::stop_active_gate(&dead, true), Some(("not_running", 0)));
    let live = wc::DaemonStatus { running: true, ws_port: 1, ..Default::default() };
    assert_eq!(wc::stop_active_gate(&live, false), Some(("no_qt", 0)));
    assert_eq!(wc::stop_active_gate(&live, true), None);
    assert_eq!(wc::stop_active_result(true, 3), ("stopped", 3));
    assert_eq!(wc::stop_active_result(true, 0), ("stopped", 0));
    assert_eq!(wc::stop_active_result(false, 2), ("error", 0));
}

// ============================================================================
// 4. Кодек запросов
// ============================================================================

#[test]
fn нумерация_request_id() {
    let mut ids = wc::RequestIds::new();
    assert_eq!(ids.peek(), 1);
    let g = golden();
    for case in g["requests"].as_array().unwrap() {
        assert_eq!(ids.alloc(), case["request_id"].as_u64().unwrap());
    }
    for case in g["generic"].as_array().unwrap() {
        assert_eq!(ids.alloc(), case["request_id"].as_u64().unwrap());
    }
    assert_eq!(ids.alloc(), g["settings_set"]["request_id"].as_u64().unwrap());
    assert_eq!(ids.peek(), g["next_id_after"].as_u64().unwrap());
}

#[test]
fn таблица_полей_совпадает_с_дескриптором_python() {
    let g = golden();
    let expect: Vec<(String, u32)> = g["request_payload_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (r[0].as_str().unwrap().to_string(), r[1].as_u64().unwrap() as u32)
        })
        .collect();
    let got: Vec<(String, u32)> = wc::REQUEST_PAYLOAD_FIELDS
        .iter()
        .map(|(n, v)| ((*n).to_string(), *v))
        .collect();
    assert_eq!(got, expect);
    assert_eq!(wc::request_payload_field_number("wallpaper_get"), Some(42));
    assert_eq!(wc::request_payload_field_number("нет_такого"), None);
}

/// Таблица генерится build.rs из `control.proto`, поэтому «отстать от вендора»
/// она больше не может — но проверить, что генерация ЖИВАЯ, надо: пустой или
/// обрезанный include прошёл бы мимо теста выше только вместе со протухшим
/// эталоном (именно так дыра и появилась: рукописный список и golden отстали
/// синхронно). Здесь — независимые от эталона инварианты.
#[test]
fn таблица_полей_генерится_из_proto() {
    // Поля, добавленные апстримом позже рукописной таблицы (69–74): их
    // отсутствие роняло `call(field, msg)` из bridge.py на Python-фолбэк.
    for (name, num) in [
        ("plugin_action", 69),
        ("subscription_status", 70),
        ("subscription_set", 71),
        ("qr_login_cancel", 72),
        ("remote_settings_patch", 73),
        ("wallpaper_unsubscribe", 74),
    ] {
        assert_eq!(
            wc::request_payload_field_number(name),
            Some(num),
            "поле {name} потерялось — генерация таблицы сломана"
        );
    }
    // Имена уникальны, номера уникальны и идут по возрастанию (как в .proto —
    // на этом порядке держится побайтовая сверка кадров).
    let mut prev = 0u32;
    let mut seen = std::collections::HashSet::new();
    for (name, num) in wc::REQUEST_PAYLOAD_FIELDS {
        assert!(*num > prev, "поле {name}: номера не по возрастанию");
        assert!(seen.insert(*name), "дубль поля {name}");
        prev = *num;
    }
    // Ни одно поле не попало в зарезервированный вендором диапазон.
    for reserved in [55, 56, 67, 68] {
        assert!(
            !wc::REQUEST_PAYLOAD_FIELDS.iter().any(|(_, n)| *n == reserved),
            "поле с зарезервированным номером {reserved}"
        );
    }
    // Каждое имя из таблицы обязано собираться `request_bytes` (то есть быть
    // валидным путём `call(field, msg)`), и кадр обязан читаться prost'ом.
    for (name, num) in wc::REQUEST_PAYLOAD_FIELDS {
        let bytes = wc::request_bytes(7, name, &[]).unwrap();
        let req = pb::Request::decode(&bytes[..]).unwrap();
        assert_eq!(req.request_id, 7);
        // Пустой payload — валидное сообщение-без-полей: prost обязан выбрать
        // именно ЭТОТ вариант oneof, иначе номер поля в таблице неверный.
        assert!(
            req.payload.is_some(),
            "поле {name} (={num}) не распознано как вариант oneof payload"
        );
    }
}

/// Сборщик кадра по имени кейса из фикстуры. `request_id` берётся из эталона —
/// сверяется ВЕСЬ кадр, включая varint номера запроса.
fn build(name: &str, id: u64) -> Vec<u8> {
    let s: Vec<String> = Vec::new();
    match name {
        "health" => wc::health(id),
        "wallpaper_list_default" => {
            wc::wallpaper_list(id, &wc::WallpaperListParams::default())
        }
        "wallpaper_list_full" => wc::wallpaper_list(
            id,
            &wc::WallpaperListParams {
                search_text: "ночь".into(),
                sort_key: pb::WallpaperSortKey::Size as i32,
                sort_desc: true,
                skip_types: vec!["video".into(), "web".into()],
                filter_tags: vec!["anime".into(), "город".into()],
                skip_content_ratings: vec![
                    "Mature".into(),
                    "Questionable".into(),
                ],
            },
        ),
        "display_list" => wc::display_list(id),
        "wallpaper_apply_plain" => wc::wallpaper_apply(id, "1234567890", &[]),
        "wallpaper_apply_displays" => {
            wc::wallpaper_apply(id, "1234567890", &wc::display_targets(&[1, 2, 4294967297]))
        }
        "wallpaper_scan" => wc::wallpaper_scan(id),
        "wallpaper_get" => wc::wallpaper_get(id, "1234567890"),
        "wallpaper_property_set" => {
            wc::wallpaper_property_set(id, "1234567890", "schemecolor", "1 0.5 0")
        }
        "wallpaper_property_reset" => {
            wc::wallpaper_property_set(id, "1234567890", "schemecolor", "")
        }
        "playlist_list" => wc::playlist_list(id),
        "playlist_status" => wc::playlist_status(id),
        "playlist_create_default" => {
            wc::playlist_create(id, "Ночной", 1, 300, &s)
        }
        "playlist_create_full" => wc::playlist_create(
            id,
            "Дневной",
            2,
            900,
            &["a".to_string(), "b".to_string(), "c".to_string()],
        ),
        "playlist_delete" => wc::playlist_delete(id, 7),
        "playlist_rename" => wc::playlist_rename(id, 7, "Новое имя"),
        "playlist_set_items" => {
            wc::playlist_set_items(id, 7, &["x".to_string(), "y".to_string()])
        }
        "playlist_set_mode" => wc::playlist_set_mode(id, 7, 2),
        "playlist_set_interval" => wc::playlist_set_interval(id, 7, 600),
        "playlist_activate_plain" => wc::playlist_activate(id, 7, &[], false),
        "playlist_activate_full" => wc::playlist_activate(id, 7, &wc::display_targets(&[3, 5]), true),
        "playlist_deactivate_plain" => wc::playlist_deactivate(id, &[], 0),
        "playlist_deactivate_full" => wc::playlist_deactivate(id, &wc::display_targets(&[3]), 9),
        "playlist_jump_to" => wc::playlist_jump_to(id, 7, "entry-1"),
        // Эталон снят с Python ДО починки (номер дисплея не слался вовсе),
        // поэтому здесь он нулевой — proto3 нули не пишет, и байты сходятся
        // один в один. Что номер теперь ДОХОДИТ, проверяет отдельный тест
        // `display_rename_несёт_номер_дисплея` ниже.
        "display_rename" => wc::display_rename(id, "DP-1", 0, "Основной", false),
        "display_rename_clear" => wc::display_rename(id, "DP-1", 0, "", true),
        "display_layout_set_empty" => wc::display_layout_set(
            id,
            &wc::DisplayLayoutParams { name: "DP-1".into(), ..Default::default() },
        ),
        "display_layout_set_id" => wc::display_layout_set(
            id,
            &wc::DisplayLayoutParams {
                name: "DP-1".into(),
                display_id: 42,
                ..Default::default()
            },
        ),
        "display_layout_set_full" => wc::display_layout_set(
            id,
            &wc::DisplayLayoutParams {
                name: "DP-1".into(),
                display_id: 42,
                fillmode: Some(2),
                align: Some(3),
                rotation: Some(1),
                location: Some((50, 25)),
                ..Default::default()
            },
        ),
        "display_layout_set_clears" => wc::display_layout_set(
            id,
            &wc::DisplayLayoutParams {
                name: "DP-1".into(),
                display_id: 42,
                clear_fillmode: true,
                clear_align: true,
                clear_rotation: true,
                clear_location: true,
                ..Default::default()
            },
        ),
        "renderer_list" => wc::renderer_list(id),
        "renderer_plugin_list" => wc::renderer_plugin_list(id),
        "renderer_kill" => wc::renderer_kill(id, "mon0"),
        "gpu_list" => wc::gpu_list(id),
        "tag_list" => wc::tag_list(id),
        "content_rating_list" => wc::content_rating_list(id),
        "source_list" => wc::source_list(id),
        "library_list" => wc::library_list(id),
        "library_add" => wc::library_add(id, "/home/tester/Обои", "we", ""),
        "library_remove" => wc::library_remove(id, 3),
        "library_auto_detect" => wc::library_auto_detect(id),
        "settings_get" => wc::settings_get(id),
        "plugin_list" => wc::plugin_list(id),
        "plugin_install" => wc::plugin_install(id, "/tmp/owe.zip"),
        "remote_availability" => wc::remote_availability(id),
        "remote_search_plain" => wc::remote_search(id, "", "", 0, "", &s),
        "remote_search_full" => wc::remote_search(
            id,
            "город",
            "popular",
            3,
            "steam",
            &["anime".to_string(), "4k".to_string()],
        ),
        "remote_details" => wc::remote_details(id, "42", "steam"),
        "remote_download" => wc::remote_download(id, "42", "steam"),
        "remote_uninstall" => wc::remote_uninstall(id, "42", ""),
        other => panic!("нет сборщика для кейса {other}"),
    }
}

#[test]
fn кадры_запросов_байт_в_байт() {
    let g = golden();
    let cases = g["requests"].as_array().unwrap();
    assert_eq!(cases.len(), 50, "фикстура усохла — эталон пересобрать");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let id = case["request_id"].as_u64().unwrap();
        let expect = hex_bytes(case["hex"].as_str().unwrap());
        assert_eq!(build(name, id), expect, "кадр {name} разошёлся с Python");
    }
}

#[test]
fn универсальный_путь_call() {
    let g = golden();
    for case in g["generic"].as_array().unwrap() {
        let field = case["field"].as_str().unwrap();
        let id = case["request_id"].as_u64().unwrap();
        let payload = hex_bytes(case["payload_hex"].as_str().unwrap());
        let expect = hex_bytes(case["hex"].as_str().unwrap());
        assert_eq!(
            wc::request_bytes(id, field, &payload).unwrap(),
            expect,
            "универсальный кадр {field} разошёлся с Python"
        );
    }
    assert!(wc::request_bytes(1, "нет_такого", &[]).is_err());
    // request_id == 0 — proto3-дефолт, поле опускается.
    assert_eq!(wc::request_bytes(0, "health", &[]).unwrap(), vec![0x52, 0x00]);
}

/// `settings_set` — единственный запрос с `map`-полем: порядок записей на
/// проводе не определён, поэтому сверка структурная (декодом обоих кадров).
#[test]
fn settings_set_структурно() {
    let g = golden();
    let c = &g["settings_set"];
    let id = c["request_id"].as_u64().unwrap();
    let global = hex_bytes(c["global_hex"].as_str().unwrap());
    let plugins: Vec<(String, Vec<u8>)> = c["plugins"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), hex_bytes(v.as_str().unwrap())))
        .collect();

    let got = wc::settings_set(id, &global, &plugins).unwrap();
    let expect = hex_bytes(c["hex"].as_str().unwrap());

    let a = pb::Request::decode(&got[..]).unwrap();
    let b = pb::Request::decode(&expect[..]).unwrap();
    assert_eq!(a, b, "settings_set разошёлся с Python по содержимому");
    assert_eq!(a.request_id, id);

    // Битые подсообщения не роняют мост, а дают ошибку.
    assert!(wc::settings_set(1, &[0xff, 0xff], &[]).is_err());
}

// ============================================================================
// Разбор ответа renderer_list (синхронная остановка WE-обоев)
// ============================================================================

/// Кадр, снятый с ЖИВОГО демона (`golden/we_live_frames.json`, тот же эталон,
/// что в `proto_parity.rs`). Смысл именно живого кадра: `stop_active_wallpapers`
/// решает по нему, что снимать перед установкой статичных обоев, — на
/// выдуманном ответе такую ветку проверять нечестно.
#[test]
fn идентификаторы_рендереров_из_живого_кадра() {
    let live: Value =
        serde_json::from_str(include_str!("golden/we_live_frames.json"))
            .unwrap();
    let raw = hex_bytes(
        live["responses"]["renderer_list"]["frame_hex"].as_str().unwrap(),
    );

    // Эталон — то, что из этого кадра читает Python
    // (`inst.renderer_id for inst in resp.renderer_list.instances`).
    let frame = pb::ServerFrame::decode(&raw[..]).unwrap();
    let Some(pb::server_frame::Kind::Response(rsp)) = frame.kind else {
        panic!("живой кадр renderer_list не response");
    };
    let Some(pb::response::Payload::RendererList(rl)) = rsp.payload else {
        panic!("живой кадр без payload renderer_list");
    };
    let expect: Vec<String> =
        rl.instances.iter().map(|i| i.renderer_id.clone()).collect();
    assert!(!expect.is_empty(), "в фикстуре нет активных рендереров");

    assert_eq!(wc::renderer_ids_from_frame(&raw), Some(expect));
}

/// Ветки «читать нечего»: у оригинала все они дают пустой список / «нечего
/// снимать», а не исключение — установка статичных обоев не должна падать
/// из-за формы ответа демона.
#[test]
fn идентификаторы_рендереров_вырожденные_случаи() {
    // Мусор вместо кадра.
    assert_eq!(wc::renderer_ids_from_frame(&[0xff, 0xff, 0xff]), None);
    // Пустой кадр — oneof kind не заполнен.
    assert_eq!(wc::renderer_ids_from_frame(&[]), None);

    // Ответ другого запроса — не наш кадр.
    let other = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 3,
            payload: Some(pb::response::Payload::Health(
                pb::HealthResponse {
                    service: "waywallen".into(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        })),
    };
    assert_eq!(wc::renderer_ids_from_frame(&other.encode_to_vec()), None);

    // Ответ БЕЗ payload (реальный кадр ошибки демона такой же).
    let empty = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 4,
            status: pb::Status::Internal as i32,
            ..Default::default()
        })),
    };
    assert_eq!(wc::renderer_ids_from_frame(&empty.encode_to_vec()), None);

    // Рендереров нет — ПУСТОЙ список, а не None: это «снимать нечего»,
    // и вызывающий обязан отличать его от «не разобрали».
    let none = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 5,
            payload: Some(pb::response::Payload::RendererList(
                pb::RendererListResponse::default(),
            )),
            ..Default::default()
        })),
    };
    assert_eq!(
        wc::renderer_ids_from_frame(&none.encode_to_vec()),
        Some(Vec::new())
    );

    // Инстанс с пустым renderer_id отбрасывается: убить его нечем, а в
    // счётчик снятых он бы попал и соврал бы про результат.
    let blank = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 6,
            payload: Some(pb::response::Payload::RendererList(
                pb::RendererListResponse {
                    renderers: vec!["a".into()],
                    instances: vec![
                        pb::RendererInstance {
                            renderer_id: String::new(),
                            ..Default::default()
                        },
                        pb::RendererInstance {
                            renderer_id: "a".into(),
                            ..Default::default()
                        },
                    ],
                },
            )),
            ..Default::default()
        })),
    };
    assert_eq!(
        wc::renderer_ids_from_frame(&blank.encode_to_vec()),
        Some(vec!["a".to_string()])
    );
}

// ============================================================================
// Починка: `display_rename` обязан нести номер дисплея
// ============================================================================

/// С v0.2.4 демон резолвит цель переименования по `display_id`, а `name`
/// держит только для старых клиентов («name is retained for older clients
/// but is not used for target resolution», фикс #79). Прежняя обёртка
/// номер не слала вовсе — псевдоним уходил в никуда.
#[test]
fn display_rename_несёт_номер_дисплея() {
    let frame = wc::display_rename(7, "DP-1", 42, "Основной", false);
    let request = pb::Request::decode(&frame[..]).expect("кадр разбирается");
    let Some(pb::request::Payload::DisplayRename(inner)) = request.payload else {
        panic!("не тот вид запроса");
    };
    assert_eq!(inner.display_id, 42, "номер дисплея потерялся");
    assert_eq!(inner.name, "DP-1");
    assert_eq!(inner.alias, "Основной");
    assert!(!inner.clear);
}

/// waywallen 0.5.0: имя библиотеки при добавлении. Как у вендорного
/// интерфейса (`library_query.cpp`, `LibraryAddQuery::reload`): без пути или
/// плагина запрос не уходит, имя обрезается по краям, пустое не шлётся —
/// поле остаётся незаданным, и имя по умолчанию выбирает демон.
#[test]
fn library_add_несёт_обрезанное_имя() {
    let req = wc::library_add_request("/обои", "we", "  Мастерская  ").expect("запрос уходит");
    assert_eq!(req.path, "/обои");
    assert_eq!(req.plugin_name, "we");
    assert_eq!(req.name.as_deref(), Some("Мастерская"));

    for blank in ["", "   ", "\t\n"] {
        let req = wc::library_add_request("/обои", "we", blank).expect("запрос уходит");
        assert_eq!(req.name, None, "пустое имя {blank:?} не шлётся");
    }
    assert_eq!(wc::library_add_request("", "we", "Имя"), None, "без пути");
    assert_eq!(wc::library_add_request("/обои", "", "Имя"), None, "без плагина");

    let frame = wc::library_add(9, "/обои", "we", " Мастерская ");
    let request = pb::Request::decode(&frame[..]).expect("кадр разбирается");
    assert_eq!(request.request_id, 9);
    let Some(pb::request::Payload::LibraryAdd(inner)) = request.payload else {
        panic!("не тот вид запроса");
    };
    assert_eq!(inner.name.as_deref(), Some("Мастерская"));
    // Без имени кадр байт в байт прежний — поле не появляется на проводе.
    assert_eq!(
        wc::library_add(9, "/обои", "we", ""),
        pb::Request {
            request_id: 9,
            payload: Some(pb::request::Payload::LibraryAdd(pb::LibraryAddRequest {
                path: "/обои".into(),
                plugin_name: "we".into(),
                name: None,
            })),
        }
        .encode_to_vec()
    );
}

/// waywallen 0.5.0: переименование библиотеки (поле 92). Кадр разбирается
/// в `LibraryRenameRequest{id, name}` с обрезанным именем; номер `<= 0` или
/// пустое после среза имя — запрос не уходит (`LibraryRenameQuery::reload`).
#[test]
fn library_rename_кадр_и_отказы() {
    assert_eq!(
        wc::request_payload_field_number("library_rename"),
        Some(92),
        "номер поля из proto 0.5.0"
    );
    let frame = wc::library_rename(11, 3, "  Обои с рабочего стола ");
    let request = pb::Request::decode(&frame[..]).expect("кадр разбирается");
    assert_eq!(request.request_id, 11);
    let Some(pb::request::Payload::LibraryRename(inner)) = request.payload else {
        panic!("не тот вид запроса");
    };
    assert_eq!(inner.id, 3);
    assert_eq!(inner.name, "Обои с рабочего стола");

    let body = wc::library_rename_request(3, " Имя ").expect("запрос уходит");
    assert_eq!(body, pb::LibraryRenameRequest { id: 3, name: "Имя".into() });
    // Универсальный путь вкладки (тело + номер поля) даёт тот же кадр.
    assert_eq!(
        wc::request_bytes(11, "library_rename", &body.encode_to_vec()).unwrap(),
        wc::library_rename(11, 3, "Имя")
    );
    assert_eq!(wc::library_rename_request(0, "Имя"), None, "номер 0");
    assert_eq!(wc::library_rename_request(-4, "Имя"), None, "отрицательный номер");
    assert_eq!(wc::library_rename_request(3, ""), None, "пустое имя");
    assert_eq!(wc::library_rename_request(3, "   "), None, "имя из пробелов");
}

// ============================================================================
// Кадр сервера для колбэков на Python
// ============================================================================
//
// Своего протобуфа у продукта больше нет: ответ демона колбэкам на Python
// разбирает ядро. Проверяется ровно то, что они читают.

#[test]
fn reply_fields_reads_health_state() {
    let frame = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 12,
            status: pb::Status::Ok as i32,
            payload: Some(pb::response::Payload::Health(pb::HealthResponse {
                service: "waywallen".into(),
                state: "healthy".into(),
                os_name: "CachyOS".into(),
            })),
            ..Default::default()
        })),
    };
    let r = wc::reply_fields(&frame.encode_to_vec()).expect("кадр разбирается");
    assert_eq!(r.kind, "response");
    assert_eq!(r.request_id, 12);
    assert_eq!(r.payload, "health");
    assert!(r.text.contains(&("state".to_string(), "healthy".to_string())));
    assert!(r.text.contains(&("os_name".to_string(), "CachyOS".to_string())));
}

#[test]
fn reply_fields_reads_plugin_install() {
    let frame = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 3,
            error_code: 0,
            message: String::new(),
            payload: Some(pb::response::Payload::PluginInstall(
                pb::PluginInstallResponse {
                    plugin_id: "org.waywallen.open-wallpaper-engine".into(),
                    needs_restart: true,
                },
            )),
            ..Default::default()
        })),
    };
    let r = wc::reply_fields(&frame.encode_to_vec()).expect("кадр разбирается");
    assert_eq!(r.payload, "plugin_install");
    assert!(r.flags.contains(&("needs_restart".to_string(), true)));
    assert_eq!(
        r.text,
        vec![(
            "plugin_id".to_string(),
            "org.waywallen.open-wallpaper-engine".to_string()
        )]
    );
}

/// Отказ демона колбэк читает по `error_code`/`message`, а не по нагрузке.
#[test]
fn reply_fields_carries_refusal() {
    let frame = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 9,
            status: pb::Status::Internal as i32,
            error_code: 7,
            message: "нет места на диске".into(),
            payload: None,
        })),
    };
    let r = wc::reply_fields(&frame.encode_to_vec()).expect("кадр разбирается");
    assert_eq!(r.error_code, 7);
    assert_eq!(r.message, "нет места на диске");
    assert_eq!(r.payload, "", "нагрузки нет — имени быть не должно");
}

/// Нагрузка, разбора которой здесь нет, обязана назваться так, чтобы её не
/// приняли за пустой ответ.
#[test]
fn reply_fields_names_unparsed_payload() {
    let frame = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Response(pb::Response {
            request_id: 1,
            payload: Some(pb::response::Payload::RendererList(
                pb::RendererListResponse::default(),
            )),
            ..Default::default()
        })),
    };
    let r = wc::reply_fields(&frame.encode_to_vec()).expect("кадр разбирается");
    assert_eq!(r.payload, wc::REPLY_PAYLOAD_UNPARSED);
    assert!(r.text.is_empty() && r.flags.is_empty());
}

/// Событие демона отличимо от ответа: колбэков у него нет, разносит его
/// транспорт.
#[test]
fn reply_fields_tells_event_from_response() {
    let frame = pb::ServerFrame {
        kind: Some(pb::server_frame::Kind::Event(pb::Event::default())),
    };
    let r = wc::reply_fields(&frame.encode_to_vec()).expect("кадр разбирается");
    assert_eq!(r.kind, "event");
    assert_eq!(r.request_id, 0);
}

/// Битый кадр обязан дать ошибку, а не «пустой ответ».
#[test]
fn reply_fields_refuses_broken_frame() {
    assert!(wc::reply_fields(&[0xff, 0xff, 0xff, 0xff]).is_err());
}

/// Английские сообщения автозапуска и транспорта — без русских букв.
#[test]
fn english_autostart_and_transport_texts_have_no_russian() {
    let _lang = i18n_pin::en();
    let cyr = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
    let mut texts = vec![
        wc::set_autostart_error_message("denied"),
        wc::transport_missing_message(),
        wc::frame_parse_error_message("bad"),
        wc::disable_plan(None, "/home/u", true, true).message,
        rexon_core::i18n::tr(rexon_core::we_query::NO_CONNECTION),
    ];
    for systemd in [true, false] {
        texts.push(match wc::enable_plan(Some("/x/waywallen"), None, "/home/u", systemd) {
            wc::EnablePlan::Systemd { message, .. } | wc::EnablePlan::Xdg { message, .. } => message,
            wc::EnablePlan::NoBinary { message } => message,
        });
    }
    if let wc::EnablePlan::NoBinary { message } = wc::enable_plan(None, None, "/home/u", true) {
        texts.push(message);
    }
    for t in &texts {
        assert!(!cyr(t), "русское в английском интерфейсе: {t}");
    }
    assert_eq!(texts[3], "Autostart disabled");
}

/// Движок комплекта заменён пакетом, а демон держит старый: перезапуск один
/// раз на пару версий; AppImage человека и неизвестные версии не трогаем.
#[test]
fn engine_refresh_restarts_once_per_version_pair() {
    use wc::EngineRefresh as R;
    let key = wc::engine_refresh_key("0.5.0", "0.5.1");
    assert_eq!(wc::engine_refresh_decision(true, "0.5.0", "waywallen 0.5.1", None), R::Restart { key: key.clone() });
    assert_eq!(wc::engine_refresh_decision(true, "0.5.0", "waywallen 0.5.1", Some(&key)), R::GiveUp);
    assert_eq!(wc::engine_refresh_decision(true, "0.5.1", "waywallen 0.5.1", None), R::Fine);
    assert_eq!(wc::engine_refresh_decision(true, "0.5.1-x86_64", "waywallen 0.5.1", None), R::Fine, "хвост сборки — не повод");
    assert_eq!(wc::engine_refresh_decision(false, "0.5.0", "waywallen 0.5.1", None), R::Fine, "чужой AppImage");
    assert_eq!(wc::engine_refresh_decision(true, "", "waywallen 0.5.1", None), R::Fine);
    assert_eq!(wc::engine_refresh_decision(true, "0.5.0", "usage: waywallen [OPTIONS]", None), R::Fine);
    let body = wc::engine_refresh_body("0.5.0", "waywallen 0.5.1");
    assert!(body.contains("0.5.0") && body.contains("0.5.1"), "{body}");
}

/// Демона нет, транспорт переподключается по кругу: одна и та же ошибка
/// сокета даёт ОДИН тост на обрыв. Следующий — после восстановления и
/// нового обрыва либо когда причина сменилась.
#[test]
fn transport_error_toast_once_per_outage() {
    let refused = "Connection refused";
    let mut gate = wc::TransportErrorToast::default();
    assert!(gate.on_error(refused), "первая ошибка обрыва — тост");
    for _ in 0..5 {
        assert!(!gate.on_error(refused), "попытки переподключения тостов не плодят");
    }
    assert!(gate.on_error("Host not found"), "другая причина — новый тост");
    assert!(!gate.on_error("Host not found"));
    assert!(gate.on_error(refused), "вернулась прежняя причина — она снова новость");
    assert!(!gate.on_error(refused));

    gate.on_connected();
    assert!(gate.on_error(refused), "соединение было и пропало снова — тост");
    assert!(!gate.on_error(refused));

    gate.on_connected();
    gate.on_connected();
    assert!(gate.on_error(refused), "повторное подключение без ошибок ничего не ломает");
}
