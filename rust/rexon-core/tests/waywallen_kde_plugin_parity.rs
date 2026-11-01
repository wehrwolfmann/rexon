//! Сверка порта `rexon/waywallen_kde_plugin.py` с живым Python.
//!
//! Эталон `golden/waywallen_kde_plugin_cases.json` снят read-only из чистого
//! Python: subprocess/shutil.which монкейпатчились — РЕАЛЬНЫЙ kpackagetool6/
//! dbus-send/qdbus6/systemctl НЕ исполнялся, только argv/таймауты/решения.
//! Абсолютный путь забандленного пакета в argv токенизирован как `{BUNDLE}`
//! (машинно-зависимый) — тест подаёт тот же токен и сверяет литерально.

use rexon_core::waywallen_kde_plugin as w;
use serde_json::Value;

mod i18n_pin;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/waywallen_kde_plugin_cases.json")).unwrap()
}

fn strvec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// Токен `{BUNDLE}` в тесте вместо машинно-зависимого пути пакета.
const BUNDLE: &str = "{BUNDLE}";

// ── окружение / константы / пути ─────────────────────────────────────────

#[test]
fn is_kde_matches_python() {
    for c in golden()["is_kde"].as_array().unwrap() {
        let env = c["env"].as_str().unwrap();
        let want = c["result"].as_bool().unwrap();
        assert_eq!(w::is_kde(env), want, "is_kde({env:?})");
    }
}

#[test]
fn constants_match_python() {
    let c = &golden()["const"];
    assert_eq!(w::PLUGIN_ID, c["PLUGIN_ID"].as_str().unwrap());
    assert_eq!(w::PLUGIN_TYPE, c["PLUGIN_TYPE"].as_str().unwrap());
    assert_eq!(w::RUNTIME_SO_NAME, c["RUNTIME_SO_NAME"].as_str().unwrap());
    assert_eq!(w::RUNTIME_SO_REL, c["RUNTIME_SO_REL"].as_str().unwrap());
    // Запасной путь обязан оканчиваться именем, по которому идёт поиск,
    // иначе две ветки поиска рантайма разъедутся.
    assert!(w::RUNTIME_SO_REL.ends_with(w::RUNTIME_SO_NAME));
}

/// Выбор рантайма среди находок: раскладка бандла в приоритете, иначе —
/// самый неглубокий путь, при равенстве — лексикографически меньший.
#[test]
fn pick_runtime_so_rel_logic() {
    let old = "contents/ui/WaywallenDisplayEmbed/libwaywallen-display-qml.so".to_string();
    let new = "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so".to_string();

    // Ничего не нашли — ответа нет.
    assert_eq!(w::pick_runtime_so_rel(&[], Some(&new)), None);
    // Единственная находка — она же и ответ, независимо от предпочтения.
    assert_eq!(w::pick_runtime_so_rel(&[old.clone()], Some(&new)).as_ref(), Some(&old));
    // Обе раскладки рядом (апгрейд поверх старой установки не подчистил
    // прежний каталог) — берём ту, что в нашем бандле.
    let both = vec![old.clone(), new.clone()];
    assert_eq!(w::pick_runtime_so_rel(&both, Some(&new)).as_ref(), Some(&new));
    assert_eq!(w::pick_runtime_so_rel(&both, Some(&old)).as_ref(), Some(&old));
    // Предпочтения нет (или оно не встретилось) — самый неглубокий путь.
    assert_eq!(w::pick_runtime_so_rel(&both, None).as_ref(), Some(&old));
    assert_eq!(w::pick_runtime_so_rel(&both, Some("contents/x.so")).as_ref(), Some(&old));
    // Равная глубина — лексикографический порядок, чтобы выбор не зависел
    // от порядка обхода каталога.
    let a = "contents/ui/A/libwaywallen-display-qml.so".to_string();
    let b = "contents/ui/B/libwaywallen-display-qml.so".to_string();
    assert_eq!(w::pick_runtime_so_rel(&[b.clone(), a.clone()], None).as_ref(), Some(&a));
}

#[test]
fn paths_match_python() {
    let p = &golden()["path_inputs"];
    let module_dir = p["module_dir"].as_str().unwrap();
    let home = p["home"].as_str().unwrap();
    assert_eq!(w::bundle_dir(module_dir), p["bundle_dir_expected"].as_str().unwrap());
    assert_eq!(w::install_dir(home), p["install_dir_expected"].as_str().unwrap());
    assert_eq!(w::version_marker(home), p["version_marker_expected"].as_str().unwrap());
}

// ── file_sha (версия рантайм-.so) ────────────────────────────────────────

#[test]
fn file_sha_matches_python_vectors() {
    for (_name, c) in golden()["file_sha_vectors"].as_object().unwrap() {
        // data сериализован через latin1 (байты 1:1); ASCII-векторы — как есть.
        let data = c["data"].as_str().unwrap().as_bytes();
        let want = c["sha"].as_str().unwrap();
        assert_eq!(w::file_sha(data), want, "file_sha({data:?})");
    }
}

#[test]
fn file_sha_known_vectors() {
    // sha256("") = e3b0c442..., sha256("abc") = ba7816bf... → первые 16 hex.
    assert_eq!(w::file_sha(b""), "e3b0c44298fc1c14");
    assert_eq!(w::file_sha(b"abc"), "ba7816bf8f01cfea");
}

#[test]
fn resolve_installed_version_logic() {
    // marker непустой → его текст (после strip).
    assert_eq!(w::resolve_installed_version(Some("  abc  "), Some("zzz")).as_deref(), Some("abc"));
    // marker пустой/None → хэш установленного .so.
    assert_eq!(w::resolve_installed_version(Some("   "), Some("zzz")).as_deref(), Some("zzz"));
    assert_eq!(w::resolve_installed_version(None, Some("zzz")).as_deref(), Some("zzz"));
    // ни marker, ни .so → None.
    assert_eq!(w::resolve_installed_version(None, None), None);
}

// ── select / current_plugins JS + argv ───────────────────────────────────

#[test]
fn select_plugin_dbus_argv_matches_python() {
    let g = &golden()["select_plugin_call"];
    assert_eq!(w::evaluate_script_argv(&w::select_plugin_js()), strvec(&g["argv"]));
    // таймаут dbus-send = 15 (у нас — контракт исполнителя, сверяем эталон).
    assert_eq!(g["timeout"].as_u64().unwrap(), 15);
}

#[test]
fn current_plugins_qdbus_argv_matches_python() {
    let g = &golden()["current_plugins_call"];
    assert_eq!(w::current_plugins_argv(), strvec(&g["argv"]));
    assert_eq!(g["timeout"].as_u64().unwrap(), 10);
}

#[test]
fn parse_current_plugins_matches_python() {
    for c in golden()["current_plugins_parse"].as_array().unwrap() {
        let stdout = c["stdout"].as_str().unwrap();
        let want = strvec(&c["parsed"]);
        assert_eq!(w::parse_current_plugins(stdout), want, "parse({stdout:?})");
    }
}

#[test]
fn all_selected_logic() {
    assert!(w::all_selected(&[w::PLUGIN_ID.to_string(), w::PLUGIN_ID.to_string()]));
    assert!(!w::all_selected(&[w::PLUGIN_ID.to_string(), "org.kde.image".to_string()]));
    assert!(!w::all_selected(&[]));
}

// ── restart plasmashell ──────────────────────────────────────────────────

#[test]
fn restart_plasmashell_cmds_match_python() {
    let g = golden();
    let want = g["restart_calls"].as_array().unwrap();
    let got = w::restart_plasmashell_cmds();
    assert_eq!(got.len(), want.len());
    for (cmd, exp) in got.iter().zip(want) {
        assert_eq!(cmd.argv, strvec(&exp["argv"]));
        assert_eq!(cmd.timeout_secs, exp["timeout"].as_u64().unwrap());
    }
}

// ── install_or_upgrade (полный конвейер kpackagetool6) ───────────────────

struct Case {
    name: &'static str,
    tool: Option<&'static str>,
    installed: bool,
    installed_ver: Option<&'static str>,
    bundle_ver: Option<&'static str>,
    rcs: Vec<i32>,
}

/// Сценарии-входы совпадают с extract_golden.py; выходы сверяются с golden.
fn install_cases() -> Vec<Case> {
    let bv = "0123456789abcdef";
    vec![
        Case { name: "fresh_install", tool: Some("kpackagetool6"), installed: false,
               installed_ver: None, bundle_ver: Some(bv), rcs: vec![0] },
        Case { name: "up_to_date", tool: Some("kpackagetool6"), installed: true,
               installed_ver: Some(bv), bundle_ver: Some(bv), rcs: vec![] },
        Case { name: "upgrade_ok", tool: Some("kpackagetool6"), installed: true,
               installed_ver: Some("old"), bundle_ver: Some(bv), rcs: vec![0] },
        Case { name: "upgrade_fallback", tool: Some("kpackagetool6"), installed: true,
               installed_ver: Some("old"), bundle_ver: Some(bv), rcs: vec![1, 0, 0] },
        Case { name: "upgrade_fallback_fail", tool: Some("kpackagetool6"), installed: true,
               installed_ver: Some("old"), bundle_ver: Some(bv), rcs: vec![1, 0, 1] },
        Case { name: "fresh_fail", tool: Some("kpackagetool6"), installed: false,
               installed_ver: None, bundle_ver: Some(bv), rcs: vec![1] },
        Case { name: "fresh_no_bundle_v", tool: Some("kpackagetool6"), installed: false,
               installed_ver: None, bundle_ver: None, rcs: vec![0] },
    ]
}

#[test]
fn install_or_upgrade_matches_python() {
    let g = golden();
    let cases = &g["install_cases"];
    for case in install_cases() {
        let exp = &cases[case.name];
        // Очередь rc, как отдавал монкейпатченный subprocess.run (stderr="boom").
        let mut queue = case.rcs.iter().copied();
        let mut run = |_argv: &[String]| {
            let rc = queue.next().unwrap_or(0);
            w::RunOutput { rc, stderr: "boom".to_string(), stdout: String::new() }
        };
        let out = w::install_or_upgrade(
            case.tool, BUNDLE, true, case.installed,
            case.installed_ver, case.bundle_ver, &mut run,
        );

        // argv + таймауты выполненных вызовов.
        let want_argv: Vec<Vec<String>> = exp["argv"]
            .as_array().unwrap().iter().map(strvec).collect();
        let got_argv: Vec<Vec<String>> = out.calls.iter().map(|c| c.argv.clone()).collect();
        assert_eq!(got_argv, want_argv, "argv [{}]", case.name);
        let want_to: Vec<u64> = exp["timeouts"].as_array().unwrap()
            .iter().map(|v| v.as_u64().unwrap()).collect();
        let got_to: Vec<u64> = out.calls.iter().map(|c| c.timeout_secs).collect();
        assert_eq!(got_to, want_to, "timeouts [{}]", case.name);

        assert_eq!(out.ok, exp["ok"].as_bool().unwrap(), "ok [{}]", case.name);
        assert_eq!(out.message, exp["msg"].as_str().unwrap(), "msg [{}]", case.name);
        assert_eq!(out.installed_now, exp["installed_now"].as_bool().unwrap(),
                   "installed_now [{}]", case.name);
        let want_marker = exp["marker_written"].as_str();
        assert_eq!(out.marker_to_write.as_deref(), want_marker, "marker [{}]", case.name);
    }
}

#[test]
fn install_no_tool_matches_python() {
    let g = golden();
    let exp = g["install_no_tool"]["result"].as_array().unwrap();
    let mut run = |_argv: &[String]| w::RunOutput {
        rc: 0, stderr: String::new(), stdout: String::new(),
    };
    let out = w::install_or_upgrade(None, BUNDLE, false, false, None, None, &mut run);
    assert_eq!(out.ok, exp[0].as_bool().unwrap());
    assert_eq!(out.message, exp[1].as_str().unwrap());
    assert_eq!(out.installed_now, exp[2].as_bool().unwrap());
    assert!(out.calls.is_empty());
}

#[test]
fn install_bundle_invalid_message() {
    // Не покрыто golden (bundle реальный существует), но логика Python явная.
    let mut run = |_argv: &[String]| w::RunOutput {
        rc: 0, stderr: String::new(), stdout: String::new(),
    };
    let out = w::install_or_upgrade(Some("kpackagetool6"), "/x/pkg", false,
                                    false, None, None, &mut run);
    assert!(!out.ok);
    assert_eq!(out.message, "забандленный пакет не найден: /x/pkg");
    assert!(out.calls.is_empty());
}

// ── оркестрация ensure (чистые решения) ──────────────────────────────────

#[test]
fn should_reselect_logic() {
    assert!(w::should_reselect(true, true));   // свежая установка → всегда
    assert!(w::should_reselect(false, false)); // не на всех выбран → да
    assert!(!w::should_reselect(false, true)); // уже выбран, не ставили → нет
}

#[test]
fn ensure_message_logic() {
    assert_eq!(w::ensure_message("плагин установлен", false, false), "плагин установлен");
    assert_eq!(w::ensure_message("плагин установлен", true, false),
               "плагин установлен; выбран на десктопах");
    assert_eq!(w::ensure_message("плагин обновлён", true, true),
               "плагин обновлён; выбран на десктопах; plasmashell перезапущен");
    assert_eq!(w::ensure_message("плагин уже установлен (актуальная версия)", false, true),
               "плагин уже установлен (актуальная версия); plasmashell перезапущен");
}

// ── назначение плагина только по применению (дефект 30.10.2026) ─────────

#[test]
fn only_apply_selects_plugin() {
    assert!(!w::EnsurePurpose::Prepare.selects(), "открытие вкладки обоев стола не меняет");
    assert!(w::EnsurePurpose::Apply.selects());
}

#[test]
fn requests_that_show_wallpaper() {
    for f in ["wallpaper_apply", "playlist_activate", "playlist_jump_to"] {
        assert!(w::request_shows_wallpaper(f), "{f}");
    }
    // Портал ставит картинку мимо дисплея waywallen; остальное обоев не показывает.
    for f in ["wallpaper_apply_via_portal", "wallpaper_list", "display_list", "playlist_list", "health"] {
        assert!(!w::request_shows_wallpaper(f), "{f}");
    }
}

#[test]
fn restart_only_after_upgrading_plugin_in_use() {
    assert!(w::restart_after_install(true, true, true));
    assert!(!w::restart_after_install(true, false, false), "свежая установка — без перезапуска");
    assert!(!w::restart_after_install(true, true, false), "обновили невыбранный — без перезапуска");
    assert!(!w::restart_after_install(false, true, true), "ничего не ставили — без перезапуска");
}

#[test]
fn any_selected_logic() {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(!w::any_selected(&v(&[])));
    assert!(!w::any_selected(&v(&["org.kde.image"])));
    assert!(w::any_selected(&v(&["org.kde.image", "org.waywallen.kde"])));
}

#[test]
fn apply_gate_holds_only_on_kde_without_displays() {
    use w::ApplyGate::{HoldAndSelect, Send};
    assert_eq!(w::apply_gate(true, "wallpaper_apply", 0), HoldAndSelect);
    assert_eq!(w::apply_gate(true, "playlist_activate", 0), HoldAndSelect);
    assert_eq!(w::apply_gate(true, "wallpaper_apply", 1), Send, "дисплей есть — плагин уже выбран");
    assert_eq!(w::apply_gate(false, "wallpaper_apply", 0), Send, "не KDE");
    assert_eq!(w::apply_gate(true, "wallpaper_list", 0), Send, "чтение библиотеки обоев не трогает");
}

#[test]
fn apply_enabled_without_displays_only_when_frame_is_held() {
    // Дефект 30.10.2026: «Применить» была серой при нуле дисплеев, а дисплей
    // появляется только после применения. На KDE кнопка доступна — кадр
    // придержится, плагин назначится.
    assert!(w::apply_without_displays(true));
    assert_eq!(
        w::apply_without_displays(true),
        w::apply_gate(true, "wallpaper_apply", 0) == w::ApplyGate::HoldAndSelect,
        "кнопка и транспорт решают одно и то же"
    );
    // Не KDE — придержки нет, кнопку не включаем: демон всё равно откажет.
    assert!(!w::apply_without_displays(false));
    assert_eq!(w::apply_gate(false, "wallpaper_apply", 0), w::ApplyGate::Send);
}

#[test]
fn no_displays_hint_follows_apply_without_displays() {
    use rexon_core::wallpaper_backend::DisplaySetup;
    // Подписи кнопок сверяются по-английски: язык окружения прогона не
    // должен решать исход (в русском окружении тест краснел).
    let _lang = i18n_pin::en();
    let hint = w::no_displays_hint(DisplaySetup::KdePlugin);
    assert!(!hint.is_empty(), "на KDE вместо «ставьте руками» — наша подсказка");
    assert!(!hint.contains("GitHub") && !hint.contains("KDE Store"), "никаких ручных установок: {hint}");
    assert!(w::no_displays_hint(DisplaySetup::None).is_empty(), "дисплей поднимает не Rexon — страница показывает своё");
    // X11 (MATE, XFCE, Cinnamon X11 — дефект 30.10.2026): вместо голого
    // «Мониторы не зарегистрированы» — что нажать; про Plasma ни слова.
    let x11 = w::no_displays_hint(DisplaySetup::RexonHost);
    assert!(x11.contains("“Apply”") && x11.contains("“Play playlist”"), "{x11}");
    assert!(!x11.contains("Plasma"), "на MATE Plasma не при чём: {x11}");
    assert_ne!(x11, hint);
    // Оба пути без дисплеев (512c9536): «Применить» у обоев и запуск
    // плейлиста — подписи как у вендорных кнопок (qsTr "Apply",
    // подсказка кнопки «Play playlist» в PlaylistListSheet.qml).
    assert!(w::play_without_displays(true), "плейлист тоже запускается без дисплеев");
    assert!(hint.contains("“Apply”"), "кнопка обоев названа: {hint}");
    assert!(hint.contains("playlist") && hint.contains("“Play playlist”"), "плейлист назван: {hint}");
}

#[test]
fn release_held_frames() {
    assert!(!w::release_held(false, false, 0, 0), "ждём назначения");
    assert!(!w::release_held(true, true, 0, 1_000), "назначили — ждём дисплея");
    assert!(w::release_held(true, true, 1, 1_000), "дисплей появился");
    assert!(w::release_held(true, false, 0, 10), "назначить не вышло — ждать нечего");
    assert!(w::release_held(false, false, 0, w::HOLD_TIMEOUT_MS), "срок вышел");
}

#[test]
fn track_displays_follows_daemon_events() {
    use rexon_core::proto::control as pb;
    use std::collections::BTreeSet;
    let ev = |p: pb::event::Payload| pb::Event { payload: Some(p) };
    let info = |id: u64| pb::DisplayInfo { display_id: id, ..Default::default() };
    let mut ids = BTreeSet::new();
    assert!(!w::track_displays(&mut ids, &ev(pb::event::Payload::DisplaySnapshot(pb::DisplaySnapshot { displays: vec![] }))));
    assert!(w::track_displays(&mut ids, &ev(pb::event::Payload::DisplayChanged(pb::DisplayChanged { display: Some(info(3)) }))));
    assert!(!w::track_displays(&mut ids, &ev(pb::event::Payload::DisplayChanged(pb::DisplayChanged { display: Some(info(3)) }))), "правка полей — не новый дисплей");
    assert!(w::track_displays(&mut ids, &ev(pb::event::Payload::DisplaySnapshot(pb::DisplaySnapshot { displays: vec![info(3), info(5)] }))));
    assert_eq!(ids.len(), 2);
    assert!(w::track_displays(&mut ids, &ev(pb::event::Payload::DisplayRemoved(pb::DisplayRemoved { display_id: 3 }))));
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec![5]);
}

// ── откат: живые обои не запустились (риск после 66a2dd5b) ──────────────

#[test]
fn select_state_for_log_does_not_claim_false_while_pending() {
    assert_eq!(w::select_state_for_log(false, false), "подъём вывода ещё не отчитался");
    assert_eq!(w::select_state_for_log(true, true), "вывод поднят только что");
    assert!(w::select_state_for_log(true, false).contains("не поднимался"));
    // Слова общие для KDE и X11: про плагин Plasma на MATE журнал не врёт.
    for (done, ok) in [(false, false), (true, true), (true, false)] {
        assert!(!w::select_state_for_log(done, ok).contains("назнач"), "{done} {ok}");
    }
}

#[test]
fn plugins_by_desktop_roundtrip() {
    let js = w::plugins_by_desktop_js();
    assert!(js.contains("ds[i].id") && js.contains("wallpaperPlugin"));
    assert!(!js.contains("wallpaperPlugin ="), "чтение ничего не назначает");
    let got = w::parse_plugins_by_desktop(" 1=org.kde.image;7=org.waywallen.kde;bad;=x;3= \n");
    assert_eq!(
        got,
        vec![
            w::DesktopPlugin { desktop_id: 1, plugin: "org.kde.image".into() },
            w::DesktopPlugin { desktop_id: 7, plugin: "org.waywallen.kde".into() },
        ]
    );
    assert!(w::parse_plugins_by_desktop("").is_empty());
}

#[test]
fn revert_only_what_we_selected() {
    let prev = |xs: &[(i64, &str)]| {
        xs.iter().map(|(id, p)| w::DesktopPlugin { desktop_id: *id, plugin: p.to_string() }).collect::<Vec<_>>()
    };
    let before = prev(&[(1, "org.kde.image"), (2, "org.waywallen.kde"), (3, "org.kde.color")]);
    let js = w::revert_plugins_js(true, Some(&before)).expect("назначили мы — откатываем");
    assert!(js.contains("'1': 'org.kde.image'") && js.contains("'3': 'org.kde.color'"), "{js}");
    assert!(!js.contains("'2'"), "стол, где плагин стоял и до того, не трогается: {js}");
    assert!(js.contains("wallpaperPlugin == 'org.waywallen.kde'"), "только столы с нашим плагином: {js}");
    // Плагин стоял до «Применить» — ничего не откатываем.
    assert_eq!(w::revert_plugins_js(false, Some(&before)), None);
    assert_eq!(w::revert_plugins_js(true, Some(&prev(&[(1, "org.waywallen.kde")]))), None);
    assert_eq!(w::revert_plugins_js(true, None), None, "снимка нет — не гадаем");
    // Странное имя плагина в JS не попадает.
    assert_eq!(w::revert_plugins_js(true, Some(&prev(&[(1, "x'; evil()")]))), None);
}

#[test]
fn apply_watch_reverts_on_daemon_error() {
    let mut watch = w::ApplyWatch::new(42);
    assert_eq!(watch.on_response(41, false, ""), w::WatchVerdict::Keep, "чужой ответ");
    assert_eq!(watch.on_response(42, false, ""), w::WatchVerdict::Revert, "рендерер не стартовал");
}

#[test]
fn apply_watch_reverts_when_renderer_dies_early() {
    let mut watch = w::ApplyWatch::new(42);
    assert_eq!(watch.on_response(42, true, "r-7"), w::WatchVerdict::Keep);
    assert_eq!(watch.on_renderer_removed("r-8"), w::WatchVerdict::Keep, "чужой рендерер");
    assert_eq!(watch.on_renderer_removed("r-7"), w::WatchVerdict::Revert, "упал на первом кадре");
    assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS - 1), w::WatchVerdict::Keep);
    assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done, "пережил окно — обои показаны");
}

#[test]
fn apply_watch_stops_on_new_apply_or_timeout() {
    let watch = w::ApplyWatch::new(42);
    assert_eq!(watch.on_new_apply(), w::WatchVerdict::Done, "человек применил другое");
    assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Keep, "ответа ещё нет — ждём");
    assert_eq!(watch.on_tick(w::HOLD_TIMEOUT_MS + w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done);
}

// ── сторож плейлиста: рендерер в ответе не назван ───────────────────────

mod playlist_watch {
    use super::w;
    use rexon_core::proto::control as pb;

    fn ev(p: pb::event::Payload) -> pb::Event {
        pb::Event { payload: Some(p) }
    }

    /// Дисплей `id`, на котором показаны рендереры `renderers`.
    fn display(id: u64, renderers: &[&str]) -> pb::Event {
        let links = renderers
            .iter()
            .map(|r| pb::DisplayLinkInfo { renderer_id: r.to_string(), z_order: 0, active: true })
            .collect();
        ev(pb::event::Payload::DisplayChanged(pb::DisplayChanged {
            display: Some(pb::DisplayInfo { display_id: id, links, ..Default::default() }),
        }))
    }

    fn snapshot(displays: &[(u64, &[&str])]) -> pb::Event {
        let displays = displays
            .iter()
            .map(|(id, rs)| pb::DisplayInfo {
                display_id: *id,
                links: rs
                    .iter()
                    .map(|r| pb::DisplayLinkInfo { renderer_id: r.to_string(), z_order: 0, active: true })
                    .collect(),
                ..Default::default()
            })
            .collect();
        ev(pb::event::Payload::DisplaySnapshot(pb::DisplaySnapshot { displays }))
    }

    fn removed(id: &str) -> pb::Event {
        ev(pb::event::Payload::RendererRemoved(pb::RendererRemoved { renderer_id: id.to_string() }))
    }

    fn announced(id: &str) -> pb::Event {
        ev(pb::event::Payload::RendererChanged(pb::RendererChanged {
            renderer: Some(pb::RendererInstance { renderer_id: id.to_string(), ..Default::default() }),
        }))
    }

    /// ВМ, llvmpipe: плейлист запущен, демон ответил OK, рендерер первого
    /// элемента упал. Демон снимает рендерер РАНЬШЕ, чем шлёт новый список
    /// дисплеев (`on_renderer_process_exit`).
    #[test]
    fn renderer_dies_after_ok_reverts() {
        let mut watch = w::ApplyWatch::new(7);
        assert_eq!(watch.on_event(&announced("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&display(3, &["r-1"])), w::WatchVerdict::Keep);
        assert_eq!(watch.on_response(7, true, ""), w::WatchVerdict::Keep, "плейлист: следим по дисплеям");
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Revert, "рендерер упал — стол чёрный");
    }

    /// Тот же сбой, но список дисплеев без связи пришёл до снятия.
    #[test]
    fn order_of_events_does_not_matter() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&snapshot(&[(3, &[])])), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Revert);
    }

    /// Рендерер упал ещё до ответа демона — решает ответ.
    #[test]
    fn renderer_dies_before_ok_reverts_on_response() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep, "ответа ещё нет");
        assert_eq!(watch.on_response(7, true, ""), w::WatchVerdict::Revert);
    }

    /// Плейлист сменил обои: прежний рендерер снят, новый уже объявлен.
    #[test]
    fn playlist_switch_is_not_a_failure() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        watch.on_response(7, true, "");
        watch.on_event(&announced("r-2"));
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done, "окно прошло — показано");
    }

    /// Два стола: упал один рендерер, другой живёт — обои на столе есть.
    #[test]
    fn other_live_renderer_keeps_wallpaper() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&snapshot(&[(3, &["r-1"]), (4, &["r-2"])]));
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&removed("r-2")), w::WatchVerdict::Revert, "не осталось ни одного");
    }

    /// Снятие рендерера, не показанного на дисплеях после отправки, —
    /// чужое (например, сирота от прошлого показа).
    #[test]
    fn foreign_renderer_removal_is_ignored() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&removed("old")), w::WatchVerdict::Keep);
        // Плейлист ждёт подходящих обоев — рендерера нет вовсе, откатывать нечего.
        assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done);
    }

    /// Рендерер назван в ответе (`wallpaper_apply`) — следим ровно за ним,
    /// как прежде.
    #[test]
    fn named_renderer_path_unchanged() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        watch.on_response(7, true, "r-1");
        assert_eq!(watch.on_event(&removed("r-9")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Revert);
    }
}

#[test]
fn play_without_displays_follows_apply_gate() {
    // Дефект 30.10.2026: плейлист на свежей KDE не запускался — без дисплея
    // кнопка серая, а пустые цели давали «уже играет».
    assert!(w::play_without_displays(true));
    assert_eq!(
        w::play_without_displays(true),
        w::apply_gate(true, "playlist_activate", 0) == w::ApplyGate::HoldAndSelect,
        "кнопка и транспорт решают одно и то же"
    );
    assert!(!w::play_without_displays(false), "не KDE — придержки нет");
}

#[test]
fn revert_texts_are_plain() {
    for text in [w::revert_done_text(), w::revert_failed_text()] {
        assert!(!text.is_empty());
        for jargon in ["renderer", "plugin", "waywallen", "spawn"] {
            assert!(!text.to_lowercase().contains(jargon), "без жаргона: {text}");
        }
    }
}

// ── сторож версии (дефект 02.10.2026) ────────────────────────────────────
//
// Rexon r1062 с вложенным плагином 0.3.3 (протокол вывода v8) поставил его
// поверх 0.4.0, и демон waywallen 0.4.3 (принимает только v9) остался без
// мониторов. Случаи — в эталоне `version_guard_cases`.

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn check_from(input: &Value) -> w::PluginCheck {
    let i = &input["installed"];
    w::PluginCheck {
        bundle_version: opt_str(&input["bundle_version"]),
        bundle_sha: opt_str(&input["bundle_sha"]),
        installed: w::InstalledPlugin {
            present: i["present"].as_bool().unwrap(),
            vendor_version: opt_str(&i["vendor_version"]),
            marker: i["marker"].as_bool().unwrap(),
            runtime_sha: opt_str(&i["runtime_sha"]),
        },
        daemon_version: opt_str(&input["daemon_version"]),
        consent: input["consent"].as_bool().unwrap(),
    }
}

#[test]
fn version_guard_cases_match_golden() {
    let g = golden();
    let cases = g["version_guard_cases"].as_object().expect("раздел version_guard_cases");
    assert!(cases.len() >= 15, "случаев сторожа подозрительно мало: {}", cases.len());
    for (name, case) in cases {
        let exp = &case["expected"];
        let plan = w::plan_plugin(&check_from(&case["input"]));
        assert_eq!(plan.action.as_str(), exp["action"].as_str().unwrap(), "действие [{name}]: {}", plan.reason);
        assert_eq!(plan.installed_version, opt_str(&exp["installed_version"]), "версия установленного [{name}]");
        assert_eq!(plan.notice.as_ref().map(|n| n.kind()), exp["notice"].as_str(), "сообщение [{name}]");
        assert_eq!(
            plan.notice.as_ref().is_some_and(|n| n.offers_install()),
            exp["offers_install"].as_bool().unwrap(),
            "кнопка «Поставить версию Rexon» [{name}]"
        );
        assert_eq!(plan.select, exp["select"].as_bool().unwrap(), "выбор на десктопах [{name}]");
        if let Some(older) = exp["bundle_older"].as_bool() {
            match &plan.notice {
                Some(w::PluginNotice::BundleIncompatible { bundle_older, .. }) => {
                    assert_eq!(*bundle_older, older, "что обновлять [{name}]")
                }
                other => panic!("[{name}] ждали bundle_incompatible, а {other:?}"),
            }
        }
    }
}

/// Главное правило отдельно от эталона: старое поверх нового — никогда,
/// ни при каком демоне и ни при каком маркере.
#[test]
fn never_installs_older_over_newer() {
    for daemon in [None, Some("0.3.9"), Some("0.4.3"), Some("9.9.9")] {
        for marker in [false, true] {
            let c = w::PluginCheck {
                bundle_version: Some("0.3.3".into()),
                bundle_sha: Some("e9c240a1da4c9327".into()),
                installed: w::InstalledPlugin {
                    present: true,
                    vendor_version: Some("0.4.0".into()),
                    marker,
                    runtime_sha: Some("bc9e0fbd268e448a".into()),
                },
                daemon_version: daemon.map(str::to_string),
                consent: false,
            };
            let plan = w::plan_plugin(&c);
            assert!(!plan.action.installs(), "старое поверх нового: демон {daemon:?}, маркер {marker}");
        }
    }
}

#[test]
fn protocol_tables_follow_upstream_tags() {
    assert_eq!(w::plugin_protocol("0.3.3"), Some(8));
    assert_eq!(w::plugin_protocol("0.4.0"), Some(9));
    assert_eq!(w::plugin_protocol("v0.2.8"), Some(7));
    assert_eq!(w::plugin_protocol("0.4.1"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.2"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.3"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.4"), None, "вне проверенной таблицы — не угадываем");
    assert_eq!(w::daemon_protocols("0.4.3"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.3.9"), Some((8, 8)));
    assert_eq!(w::daemon_protocols("0.4.4"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.4.5"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.5.0"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.5.1"), None);
    assert_eq!(w::compatible_with_daemon(Some("0.4.3"), Some("0.5.0")), Some(true));
    assert_eq!(w::daemon_protocols("0.2.6"), None);
    assert_eq!(w::compatible_with_daemon(Some("0.3.3"), Some("0.4.3")), Some(false));
    assert_eq!(w::compatible_with_daemon(Some("0.4.0"), Some("0.4.3")), Some(true));
    assert_eq!(w::compare_versions("0.4", "0.4.0"), Some(std::cmp::Ordering::Equal));
    assert_eq!(w::compare_versions("0.10.0", "0.9.9"), Some(std::cmp::Ordering::Greater));
    assert_eq!(w::compare_versions("", "0.1"), None);
}

/// Сторож вендоринга: вложенный плагин обязан быть в проверенной таблице
/// протоколов, иначе сверка с демоном молча выключится. Обновили плагин —
/// допишите строку в `PLUGIN_DISPLAY_PROTOCOLS` по тегу апстрима.
#[test]
fn bundled_plugin_version_is_in_protocol_table() {
    let toml = include_str!("../../../data/kpackage/org.waywallen.kde/vendor.toml");
    let v = w::parse_vendor_version(toml).expect("version в vendor.toml бандла");
    assert!(
        w::plugin_protocol(&v).is_some(),
        "плагин {v} вне таблицы протоколов (проверена до {})",
        w::PLUGIN_PROTOCOLS_CHECKED_UP_TO
    );
    // Хэш рантайма бандла узнаётся — чужая копия того же релиза не станет
    // «неизвестной версии». Плазмоида в git нет (архив выпуска качает
    // пакет); в дереве он есть, только если его распаковал
    // `vendor-check --выкачать`, — иначе говорим «не проверено» вслух.
    let so_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/kpackage/org.waywallen.kde/contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so"
    );
    match std::fs::read(so_path) {
        Ok(so) => assert_eq!(w::version_from_runtime_sha(&w::file_sha(&so)), Some(v.as_str())),
        Err(_) => eprintln!(
            "НЕ ПРОВЕРЕНО: плазмоид не распакован в дереве — \
             `cargo run -p rexon-core --bin vendor-check -- --выкачать`"
        ),
    }
}

#[test]
fn parse_vendor_version_cases() {
    assert_eq!(w::parse_vendor_version("name = \"x\"\nversion     = \"0.4.0\"\n").as_deref(), Some("0.4.0"));
    assert_eq!(w::parse_vendor_version("version = \"\"\n"), None);
    assert_eq!(w::parse_vendor_version("note = \"\"\"\nversion = \"9\"\n\"\"\"\n"), None);
    assert_eq!(w::parse_vendor_version("не toml ["), None);
}

/// Тексты для человека: английский исходник с подставленными версиями.
#[test]
fn notice_texts_name_versions() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let t = w::PluginNotice::BundleIncompatible {
        bundle_version: "0.3.3".into(),
        daemon_version: "0.4.3".into(),
        bundle_older: true,
    }
    .text();
    assert!(t.contains("0.4.3") && t.contains("0.3.3") && t.contains("Update Rexon"), "{t}");
    let t = w::PluginNotice::InstalledIncompatible {
        plugin_version: "0.4.0".into(),
        daemon_version: "0.3.9".into(),
        offer_install: false,
        plugin_older: false,
    }
    .text();
    assert!(t.contains("0.4.0") && t.contains("0.3.9") && t.ends_with("Update waywallen."), "{t}");
    assert!(!w::PluginNotice::ForeignUnknown.text().is_empty());
    assert_eq!(w::install_rexon_version_label(), "Install Rexon's Version");
}

/// Каждая новая строка сторожа переведена на русский и немецкий.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po настоящим msgfmt")]
fn notice_texts_translated_ru_de() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let msgids = [
        w::PluginNotice::ForeignUnknown.text(),
        "The installed Plasma wallpaper plugin (version %1) does not work with the waywallen \
         engine %2, so live wallpapers are not shown."
            .to_string(),
        "Rexon can install a version that fits.".to_string(),
        "Update Rexon to get a newer plugin.".to_string(),
        "Update waywallen.".to_string(),
        "The waywallen engine %1 is newer than the Plasma wallpaper plugin in this version of \
         Rexon (%2). Update Rexon, otherwise live wallpapers will not appear on KDE."
            .to_string(),
        "The Plasma wallpaper plugin in Rexon (%2) needs a newer waywallen engine than the \
         installed one (%1). Update waywallen."
            .to_string(),
        w::install_rexon_version_label(),
        "Could not install the Plasma wallpaper plugin: %1".to_string(),
    ];
    for lang in ["ru", "de"] {
        let po = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../po/{lang}.po"));
        let out = std::env::temp_dir().join(format!("rexon-wkp-{lang}-{}.mo", std::process::id()));
        let status = std::process::Command::new("msgfmt")
            .arg("--check")
            .arg("-o")
            .arg(&out)
            .arg(&po)
            .status()
            .expect("msgfmt (пакет gettext) запускается");
        assert!(status.success(), "msgfmt отверг {}", po.display());
        let catalog = rexon_core::i18n::Catalog::load(&out).expect("каталог читается");
        let _ = std::fs::remove_file(&out);
        for msgid in &msgids {
            let got = catalog.get(None, msgid);
            assert!(got.is_some_and(|s| !s.is_empty()), "{lang}: нет перевода «{msgid}»");
        }
    }
}

/// Хост-клиент вывода на X11 находит элемент вывода в том же пакете, что
/// Rexon везёт для Plasma: вшитый — первым, установленный — запасным.
#[test]
fn display_item_found_next_to_runtime() {
    let so = "/usr/lib/rexon/rexon/kpackage/org.waywallen.kde/contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so";
    assert_eq!(
        w::display_item_qml_of(so).as_deref(),
        Some("/usr/lib/rexon/rexon/kpackage/org.waywallen.kde/contents/ui/Plugin/PluginDisplay.qml")
    );
    assert_eq!(
        w::display_package_candidates(Some("/usr/lib/rexon/rexon"), "/home/u"),
        vec![
            "/usr/lib/rexon/rexon/kpackage/org.waywallen.kde".to_string(),
            "/home/u/.local/share/plasma/wallpapers/org.waywallen.kde".to_string(),
        ]
    );
    assert!(w::display_package_candidates(None, "").is_empty());
    // Вшитый пакет дерева разработчика: обёртка и модуль на месте.
    let tree = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data");
    let base = w::display_package_candidates(Some(tree), "")[0].clone();
    if let Some(so) = w::find_runtime_so(&base, Some(w::RUNTIME_SO_REL)) {
        let qml = w::display_item_qml_of(&so).unwrap();
        assert!(std::path::Path::new(&qml).is_file(), "{qml}");
    }
}
