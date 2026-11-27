//! Check of the `rexon/waywallen_kde_plugin.py` port against live Python.
//!
//! The reference `golden/waywallen_kde_plugin_cases.json` was captured
//! read-only from pure Python: subprocess/shutil.which were monkeypatched — the
//! REAL kpackagetool6/dbus-send/qdbus6/systemctl were NOT executed, only
//! argv/timeouts/decisions. The absolute path of the bundled package in argv
//! is tokenised as `{BUNDLE}` (machine-dependent) — the test passes the same
//! token and compares literally.

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

/// The `{BUNDLE}` token in the test instead of the machine-dependent package path.
const BUNDLE: &str = "{BUNDLE}";

// ── environment / constants / paths ─────────────────────────────────────────

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
    // The fallback path must end with the name being searched for, otherwise the
    // two runtime search branches would diverge.
    assert!(w::RUNTIME_SO_REL.ends_with(w::RUNTIME_SO_NAME));
}

/// Choosing the runtime among the finds: the bundle layout has priority,
/// otherwise the shallowest path, on a tie — the lexicographically smaller one.
#[test]
fn pick_runtime_so_rel_logic() {
    let old = "contents/ui/WaywallenDisplayEmbed/libwaywallen-display-qml.so".to_string();
    let new = "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so".to_string();

    // Nothing found — no answer.
    assert_eq!(w::pick_runtime_so_rel(&[], Some(&new)), None);
    // The only find is the answer, regardless of the preference.
    assert_eq!(w::pick_runtime_so_rel(&[old.clone()], Some(&new)).as_ref(), Some(&old));
    // Both layouts side by side (an upgrade over an old install did not clean up
    // the former directory) — take the one in our bundle.
    let both = vec![old.clone(), new.clone()];
    assert_eq!(w::pick_runtime_so_rel(&both, Some(&new)).as_ref(), Some(&new));
    assert_eq!(w::pick_runtime_so_rel(&both, Some(&old)).as_ref(), Some(&old));
    // No preference (or it was not found) — the shallowest path.
    assert_eq!(w::pick_runtime_so_rel(&both, None).as_ref(), Some(&old));
    assert_eq!(w::pick_runtime_so_rel(&both, Some("contents/x.so")).as_ref(), Some(&old));
    // Equal depth — lexicographic order, so the choice does not depend on the
    // directory traversal order.
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

// ── file_sha (runtime .so version) ────────────────────────────────────────

#[test]
fn file_sha_matches_python_vectors() {
    for (_name, c) in golden()["file_sha_vectors"].as_object().unwrap() {
        // data is serialised via latin1 (bytes 1:1); ASCII vectors — as is.
        let data = c["data"].as_str().unwrap().as_bytes();
        let want = c["sha"].as_str().unwrap();
        assert_eq!(w::file_sha(data), want, "file_sha({data:?})");
    }
}

#[test]
fn file_sha_known_vectors() {
    // sha256("") = e3b0c442..., sha256("abc") = ba7816bf... → the first 16 hex.
    assert_eq!(w::file_sha(b""), "e3b0c44298fc1c14");
    assert_eq!(w::file_sha(b"abc"), "ba7816bf8f01cfea");
}

#[test]
fn resolve_installed_version_logic() {
    // marker non-empty → its text (after strip).
    assert_eq!(w::resolve_installed_version(Some("  abc  "), Some("zzz")).as_deref(), Some("abc"));
    // marker empty/None → hash of the installed .so.
    assert_eq!(w::resolve_installed_version(Some("   "), Some("zzz")).as_deref(), Some("zzz"));
    assert_eq!(w::resolve_installed_version(None, Some("zzz")).as_deref(), Some("zzz"));
    // neither marker nor .so → None.
    assert_eq!(w::resolve_installed_version(None, None), None);
}

// ── select / current_plugins JS + argv ───────────────────────────────────

#[test]
fn select_plugin_dbus_argv_matches_python() {
    let g = &golden()["select_plugin_call"];
    assert_eq!(w::evaluate_script_argv(&w::select_plugin_js()), strvec(&g["argv"]));
    // dbus-send timeout = 15 (for us — an executor contract, we compare with the reference).
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

// ── install_or_upgrade (the full kpackagetool6 pipeline) ───────────────────

struct Case {
    name: &'static str,
    tool: Option<&'static str>,
    installed: bool,
    installed_ver: Option<&'static str>,
    bundle_ver: Option<&'static str>,
    rcs: Vec<i32>,
}

/// Input scenarios match extract_golden.py; outputs are compared with golden.
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
        // rc queue, as the monkeypatched subprocess.run returned it (stderr="boom").
        let mut queue = case.rcs.iter().copied();
        let mut run = |_argv: &[String]| {
            let rc = queue.next().unwrap_or(0);
            w::RunOutput { rc, stderr: "boom".to_string(), stdout: String::new() }
        };
        let out = w::install_or_upgrade(
            case.tool, BUNDLE, true, case.installed,
            case.installed_ver, case.bundle_ver, &mut run,
        );

        // argv + timeouts of executed calls.
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
    // Not covered by golden (the real bundle exists), but the Python logic is explicit.
    let mut run = |_argv: &[String]| w::RunOutput {
        rc: 0, stderr: String::new(), stdout: String::new(),
    };
    let out = w::install_or_upgrade(Some("kpackagetool6"), "/x/pkg", false,
                                    false, None, None, &mut run);
    assert!(!out.ok);
    assert_eq!(out.message, "the bundled plugin package was not found: /x/pkg");
    assert!(out.calls.is_empty());
}

// ── ensure orchestration (pure decisions) ──────────────────────────────────

#[test]
fn should_reselect_logic() {
    assert!(w::should_reselect(true, true));   // fresh install → always
    assert!(w::should_reselect(false, false)); // not selected everywhere → yes
    assert!(!w::should_reselect(false, true)); // already selected, not installed → no
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

// ── assigning the plugin only on apply (defect of 30.10.2026) ─────────

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
    // The portal sets a picture bypassing the waywallen display; the rest does not show wallpapers.
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

// ── quiet refresh of the installed plugin (defect of 12.11.2026) ───────────

/// Plugin 0.4.x of 06.10 on the person's machine, bundle 0.4.5, daemon 0.6.0.
fn refresh_check(present: bool, installed_sha: &str) -> w::PluginCheck {
    w::PluginCheck {
        bundle_version: Some("0.4.5".into()),
        bundle_sha: Some("bbbbbbbbbbbbbbbb".into()),
        installed: if present {
            w::InstalledPlugin {
                present: true,
                vendor_version: Some("0.4.0".into()),
                marker: true,
                runtime_sha: Some(installed_sha.into()),
            }
        } else {
            w::InstalledPlugin::default()
        },
        daemon_version: Some("0.6.0".into()),
        consent: false,
        ..Default::default()
    }
}

#[test]
fn refresh_upgrades_installed_outdated_plugin() {
    let d = w::refresh_decision(&refresh_check(true, "aaaaaaaaaaaaaaaa"), true, true);
    assert_eq!(d, w::RefreshDecision::Upgrade);
    assert!(d.upgrades());
    // The daemon is not known yet (Rexon start) — the guard does not prevent the update.
    let mut c = refresh_check(true, "aaaaaaaaaaaaaaaa");
    c.daemon_version = None;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::Upgrade);
}

#[test]
fn refresh_never_installs_missing_plugin() {
    for (tool, bundle) in [(true, true), (false, true), (true, false), (false, false)] {
        let d = w::refresh_decision(&refresh_check(false, ""), tool, bundle);
        assert_eq!(d, w::RefreshDecision::NotInstalled, "инструмент {tool}, бандл {bundle}");
        assert!(!d.upgrades());
    }
}

#[test]
fn refresh_keeps_matching_plugin() {
    // The same runtime — nothing, even without the tool and the bundle (the cheap path).
    for (tool, bundle) in [(true, true), (false, false)] {
        let d = w::refresh_decision(&refresh_check(true, "bbbbbbbbbbbbbbbb"), tool, bundle);
        assert_eq!(d, w::RefreshDecision::UpToDate, "инструмент {tool}, бандл {bundle}");
    }
}

#[test]
fn refresh_skips_without_kpackagetool_or_bundle() {
    let c = refresh_check(true, "aaaaaaaaaaaaaaaa");
    assert_eq!(w::refresh_decision(&c, false, true), w::RefreshDecision::NoTool);
    assert_eq!(w::refresh_decision(&c, true, false), w::RefreshDecision::NoBundle);
}

#[test]
fn refresh_respects_version_guard() {
    // The installed one is newer than the bundle — leave it alone.
    let mut c = refresh_check(true, "aaaaaaaaaaaaaaaa");
    c.installed.vendor_version = Some("0.4.9".into());
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::Skip(w::PluginAction::KeepNewer));
    // A foreign one of unknown version — not changed silently (the tab asks).
    let mut c = refresh_check(true, "aaaaaaaaaaaaaaaa");
    c.installed.vendor_version = None;
    c.installed.marker = false;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::Skip(w::PluginAction::AskForeign));
    // Consent from a past check does not extend to a silent refresh.
    c.consent = true;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::Skip(w::PluginAction::AskForeign));
}

#[test]
fn refresh_restarts_plasmashell_only_on_kde_when_in_use() {
    assert!(w::refresh_restarts(true, true, true));
    assert!(!w::refresh_restarts(false, true, true), "не KDE — оболочку не трогаем");
    assert!(!w::refresh_restarts(true, true, false), "обновили невыбранный — без перезапуска");
    assert!(!w::refresh_restarts(true, false, true), "ничего не ставили — без перезапуска");
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
    // Defect of 30.10.2026: «Apply» was greyed out with zero displays, and a
    // display appears only after applying. On KDE the button is enabled — the
    // frame will be held, the plugin will be assigned.
    assert!(w::apply_without_displays(true));
    assert_eq!(
        w::apply_without_displays(true),
        w::apply_gate(true, "wallpaper_apply", 0) == w::ApplyGate::HoldAndSelect,
        "кнопка и транспорт решают одно и то же"
    );
    // Not KDE — no holding, the button is not enabled: the daemon would refuse anyway.
    assert!(!w::apply_without_displays(false));
    assert_eq!(w::apply_gate(false, "wallpaper_apply", 0), w::ApplyGate::Send);
}

#[test]
fn no_displays_hint_follows_apply_without_displays() {
    use rexon_core::wallpaper_backend::DisplaySetup;
    // Button labels are compared in English: the language of the run environment
    // must not decide the outcome (in a Russian environment the test went red).
    let _lang = i18n_pin::en();
    let hint = w::no_displays_hint(DisplaySetup::KdePlugin);
    assert!(!hint.is_empty(), "на KDE вместо «ставьте руками» — наша подсказка");
    assert!(!hint.contains("GitHub") && !hint.contains("KDE Store"), "никаких ручных установок: {hint}");
    assert!(w::no_displays_hint(DisplaySetup::None).is_empty(), "дисплей поднимает не Rexon — страница показывает своё");
    // X11 (MATE, XFCE, Cinnamon X11 — defect of 30.10.2026): instead of a bare
    // «No monitors registered» — what to press; not a word about Plasma.
    let x11 = w::no_displays_hint(DisplaySetup::RexonHost);
    assert!(x11.contains("“Apply”") && x11.contains("“Play playlist”"), "{x11}");
    assert!(!x11.contains("Plasma"), "на MATE Plasma не при чём: {x11}");
    assert_ne!(x11, hint);
    // Both paths without displays (512c9536): wallpaper «Apply» and starting a
    // playlist — labels as on the vendor buttons (qsTr "Apply", the tooltip of
    // the «Play playlist» button in PlaylistListSheet.qml).
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

// ── rollback: live wallpapers did not start (risk after 66a2dd5b) ──────────────

#[test]
fn select_state_for_log_does_not_claim_false_while_pending() {
    assert_eq!(w::select_state_for_log(false, false), "подъём вывода ещё не отчитался");
    assert_eq!(w::select_state_for_log(true, true), "вывод поднят только что");
    assert!(w::select_state_for_log(true, false).contains("не поднимался"));
    // Words shared by KDE and X11: the log does not lie about the Plasma plugin on MATE.
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
    // The plugin was there before «Apply» — nothing is rolled back.
    assert_eq!(w::revert_plugins_js(false, Some(&before)), None);
    assert_eq!(w::revert_plugins_js(true, Some(&prev(&[(1, "org.waywallen.kde")]))), None);
    assert_eq!(w::revert_plugins_js(true, None), None, "снимка нет — не гадаем");
    // A strange plugin name does not get into JS.
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

// ── playlist guard: the renderer is not named in the response ───────────────────────

mod playlist_watch {
    use super::w;
    use rexon_core::proto::control as pb;

    fn ev(p: pb::event::Payload) -> pb::Event {
        pb::Event { payload: Some(p) }
    }

    /// Display `id` on which the renderers `renderers` are shown.
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

    /// VM, llvmpipe: the playlist started, the daemon answered OK, the renderer of
    /// the first item crashed. The daemon removes the renderer BEFORE it sends the
    /// new display list (`on_renderer_process_exit`).
    #[test]
    fn renderer_dies_after_ok_reverts() {
        let mut watch = w::ApplyWatch::new(7);
        assert_eq!(watch.on_event(&announced("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&display(3, &["r-1"])), w::WatchVerdict::Keep);
        assert_eq!(watch.on_response(7, true, ""), w::WatchVerdict::Keep, "плейлист: следим по дисплеям");
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Revert, "рендерер упал — стол чёрный");
    }

    /// The same failure, but the display list without the link arrived before the removal.
    #[test]
    fn order_of_events_does_not_matter() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&snapshot(&[(3, &[])])), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Revert);
    }

    /// The renderer crashed even before the daemon's response — the response decides.
    #[test]
    fn renderer_dies_before_ok_reverts_on_response() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep, "ответа ещё нет");
        assert_eq!(watch.on_response(7, true, ""), w::WatchVerdict::Revert);
    }

    /// The playlist changed the wallpaper: the former renderer was removed, the new one already announced.
    #[test]
    fn playlist_switch_is_not_a_failure() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&display(3, &["r-1"]));
        watch.on_response(7, true, "");
        watch.on_event(&announced("r-2"));
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done, "окно прошло — показано");
    }

    /// Two desktops: one renderer crashed, the other lives — there is a wallpaper on the desktop.
    #[test]
    fn other_live_renderer_keeps_wallpaper() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_event(&snapshot(&[(3, &["r-1"]), (4, &["r-2"])]));
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&removed("r-1")), w::WatchVerdict::Keep);
        assert_eq!(watch.on_event(&removed("r-2")), w::WatchVerdict::Revert, "не осталось ни одного");
    }

    /// Removal of a renderer not shown on the displays after sending is foreign
    /// (e.g. an orphan from a past show).
    #[test]
    fn foreign_renderer_removal_is_ignored() {
        let mut watch = w::ApplyWatch::new(7);
        watch.on_response(7, true, "");
        assert_eq!(watch.on_event(&removed("old")), w::WatchVerdict::Keep);
        // The playlist is waiting for suitable wallpapers — there is no renderer at all, nothing to roll back.
        assert_eq!(watch.on_tick(w::FIRST_FRAME_WATCH_MS), w::WatchVerdict::Done);
    }

    /// The renderer is named in the response (`wallpaper_apply`) — we watch exactly
    /// it, as before.
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
    // Defect of 30.10.2026: a playlist on a fresh KDE did not start — without a
    // display the button was grey, and empty targets gave «already playing».
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

// ── version guard (defect of 02.10.2026) ────────────────────────────────────
//
// Rexon r1062 with the bundled plugin 0.3.3 (output protocol v8) installed it
// over 0.4.0, and the waywallen 0.4.3 daemon (accepts only v9) was left
// without monitors. The cases are in the `version_guard_cases` reference.

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
        // Cases written before the packaged plugin: no package, a bundle, no failure.
        system: w::SystemPlugin {
            present: input["system"]["present"].as_bool().unwrap_or(false),
            dir: opt_str(&input["system"]["dir"]),
            version: opt_str(&input["system"]["version"]),
            package: opt_str(&input["system"]["package"]),
        },
        bundle_missing: input["bundle_missing"].as_bool().unwrap_or(false),
        package_failed: input["package_failed"].as_bool().unwrap_or(false),
    }
}

fn assert_plan_matches(section: &str, min_cases: usize) {
    let g = golden();
    let cases = g[section].as_object().unwrap_or_else(|| panic!("section {section}"));
    assert!(cases.len() >= min_cases, "{section}: suspiciously few cases: {}", cases.len());
    for (name, case) in cases {
        let exp = &case["expected"];
        let plan = w::plan_plugin(&check_from(&case["input"]));
        assert_eq!(plan.action.as_str(), exp["action"].as_str().unwrap(), "action [{name}]: {}", plan.reason);
        assert_eq!(plan.installed_version, opt_str(&exp["installed_version"]), "installed version [{name}]");
        assert_eq!(plan.notice.as_ref().map(|n| n.kind()), exp["notice"].as_str(), "notice [{name}]");
        assert_eq!(
            plan.notice.as_ref().is_some_and(|n| n.offers_install()),
            exp["offers_install"].as_bool().unwrap(),
            "banner button [{name}]"
        );
        assert_eq!(plan.select, exp["select"].as_bool().unwrap(), "selection on desktops [{name}]");
    }
}

/// The packaged plugin `plasma-wallpaper-waywallen` (owner's decision of 25.11.2026).
#[test]
fn system_package_cases_match_golden() {
    assert_plan_matches("system_package_cases", 15);
}

/// The main rule of the packaged plugin: with it present the bundle is NEVER installed
/// (no `kpackagetool6 -i/-u`), with any copy, consent, daemon or bundle.
#[test]
fn packaged_plugin_never_gets_overwritten() {
    let copies = [
        w::InstalledPlugin::default(),
        w::InstalledPlugin { present: true, vendor_version: Some("0.4.5".into()), marker: true, runtime_sha: None },
        w::InstalledPlugin { present: true, vendor_version: None, marker: false, runtime_sha: Some("ffff".into()) },
    ];
    for installed in copies {
        for consent in [false, true] {
            for daemon in [None, Some("0.3.9"), Some("0.6.1"), Some("9.9.9")] {
                for bundle_missing in [false, true] {
                    let c = w::PluginCheck {
                        bundle_version: (!bundle_missing).then(|| "0.4.5".into()),
                        bundle_sha: (!bundle_missing).then(|| "4d90629e44055113".into()),
                        installed: installed.clone(),
                        daemon_version: daemon.map(str::to_string),
                        consent,
                        system: w::SystemPlugin { present: true, version: Some("0.4.6".into()), ..Default::default() },
                        bundle_missing,
                        package_failed: false,
                    };
                    let plan = w::plan_plugin(&c);
                    assert!(
                        !plan.action.installs() && plan.action != w::PluginAction::InstallPackage,
                        "{:?} over the packaged plugin: {c:?}",
                        plan.action
                    );
                    assert!(!w::refresh_decision(&c, true, !bundle_missing).upgrades(), "refresh over the package: {c:?}");
                }
            }
        }
    }
}

#[test]
fn refresh_moves_our_copy_away_from_package() {
    let mut c = refresh_check(true, "aaaa");
    c.installed.vendor_version = Some("0.4.5".into());
    c.system.present = true;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::RemoveUserCopy);
    assert_eq!(w::refresh_decision(&c, false, false), w::RefreshDecision::RemoveUserCopy, "no kpackagetool6 needed");
    c.installed.vendor_version = None;
    c.installed.marker = false;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::UpToDate, "a foreign copy only by consent");
    c.installed.present = false;
    assert_eq!(w::refresh_decision(&c, true, true), w::RefreshDecision::NotInstalled);
}

#[test]
fn user_copy_backup_lies_outside_plasma_wallpapers() {
    let dir = w::user_copy_backup_dir("/home/u/", "20261125-210000");
    assert_eq!(dir, "/home/u/.local/share/waywallen/plasmoid-org.waywallen.kde.bak-20261125-210000");
    assert!(!dir.contains("plasma/wallpapers"), "Plasma would still load a copy from there");
}

/// Texts of the packaged-plugin notices name the package and the versions; the
/// buttons have their own labels.
#[test]
fn package_notice_texts_and_buttons() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let t = w::PluginNotice::SystemIncompatible {
        plugin_version: "0.3.3".into(),
        daemon_version: "0.6.1".into(),
        plugin_older: true,
    }
    .text();
    assert!(t.contains("plasma-wallpaper-waywallen") && t.contains("0.3.3") && t.contains("0.6.1"), "{t}");
    assert!(t.ends_with("Update the plasma-wallpaper-waywallen package."), "{t}");
    let pkg = || w::PACKAGE_NAME.to_string();
    let t = w::PluginNotice::PackageInstalling { package: pkg(), stage: "Building".into() }.text();
    assert!(t.contains("plasma-wallpaper-waywallen") && t.ends_with("Stage: Building"), "{t}");
    assert!(!w::PluginNotice::PackageInstalling { package: pkg(), stage: String::new() }.text().contains("Stage"));
    let t = w::PluginNotice::PackageMissing { package: pkg(), reason: "no AUR helper".into() }.text();
    assert!(t.contains("plasma-wallpaper-waywallen") && t.ends_with("no AUR helper"), "{t}");
    assert!(!w::PluginNotice::PackageMissing { package: pkg(), reason: String::new() }.text().is_empty());
    let x11 = w::PluginNotice::PackageMissing { package: w::DISPLAY_MODULE_PACKAGE.into(), reason: "x".into() };
    assert!(x11.text().contains("waywallen-display"));
    assert_eq!(x11.button_label(), None, "the banner button retries only the Plasma plugin package");
    assert!(w::PluginNotice::ShadowsSystem.text().contains("plasma-wallpaper-waywallen"));
    assert_eq!(w::PluginNotice::ShadowsSystem.button_label().as_deref(), Some("Use the Packaged Version"));
    assert_eq!(
        w::PluginNotice::PackageMissing { package: pkg(), reason: String::new() }.button_label().as_deref(),
        Some("Install the Plugin Package")
    );
    assert_eq!(w::PluginNotice::ForeignUnknown.button_label(), Some(w::install_rexon_version_label()));
    assert_eq!(w::PluginNotice::PackageInstalling { package: pkg(), stage: String::new() }.button_label(), None);
    assert_eq!(
        w::PluginNotice::SystemIncompatible { plugin_version: String::new(), daemon_version: String::new(), plugin_older: false }
            .button_label(),
        None,
        "nothing Rexon could install over a package"
    );
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

/// The main rule separately from the reference: old over new — never, with
/// any daemon and any marker.
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
                ..Default::default()
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
    assert_eq!(w::plugin_protocol("0.4.4"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.5"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.6"), Some(9), "v0.4.6 checked by the tag header");
    assert_eq!(w::plugin_protocol("0.4.7"), Some(9));
    assert_eq!(w::plugin_protocol("0.4.8"), Some(9), "v0.4.8 checked by the tag header");
    assert_eq!(w::plugin_protocol("0.4.9"), None, "outside the checked table: no guessing");
    assert_eq!(w::daemon_protocols("0.4.3"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.3.9"), Some((8, 8)));
    assert_eq!(w::daemon_protocols("0.4.4"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.4.5"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.5.0"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.5.1"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.6.0"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.6.1"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.6.2"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.6.3"), Some((9, 9)));
    assert_eq!(w::daemon_protocols("0.6.4"), None);
    assert_eq!(w::compatible_with_daemon(Some("0.4.8"), Some("0.6.3")), Some(true));
    assert_eq!(w::compatible_with_daemon(Some("0.4.5"), Some("0.6.1")), Some(true));
    assert_eq!(w::compatible_with_daemon(Some("0.4.3"), Some("0.5.0")), Some(true));
    assert_eq!(w::compatible_with_daemon(Some("0.4.5"), Some("0.6.0")), Some(true));
    assert_eq!(w::daemon_protocols("0.2.6"), None);
    assert_eq!(w::compatible_with_daemon(Some("0.3.3"), Some("0.4.3")), Some(false));
    assert_eq!(w::compatible_with_daemon(Some("0.4.0"), Some("0.4.3")), Some(true));
    assert_eq!(w::compare_versions("0.4", "0.4.0"), Some(std::cmp::Ordering::Equal));
    assert_eq!(w::compare_versions("0.10.0", "0.9.9"), Some(std::cmp::Ordering::Greater));
    assert_eq!(w::compare_versions("", "0.1"), None);
}

/// Vendoring guard: the bundled plugin must be in the verified protocol
/// table, otherwise the check against the daemon silently turns off. Updated
/// the plugin — add a line to `PLUGIN_DISPLAY_PROTOCOLS` by the upstream tag.
#[test]
fn bundled_plugin_version_is_in_protocol_table() {
    let toml = include_str!("../../../data/kpackage/org.waywallen.kde/vendor.toml");
    let v = w::parse_vendor_version(toml).expect("version в vendor.toml бандла");
    assert!(
        w::plugin_protocol(&v).is_some(),
        "плагин {v} вне таблицы протоколов (проверена до {})",
        w::PLUGIN_PROTOCOLS_CHECKED_UP_TO
    );
    // The bundle runtime hash is recognised — a foreign copy of the same release
    // will not become «unknown version». The plasmoid is not in git (the package
    // downloads the release archive); it is in the tree only if
    // `vendor-check --fetch` unpacked it — otherwise we say «not checked» out loud.
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

/// Texts for the person: the English source with versions substituted.
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

/// Every new guard string is translated into Russian and German.
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
        // The packaged plugin (25.11.2026).
        "Use the Packaged Version".to_string(),
        "Install the Plugin Package".to_string(),
        "The installed %1 package (version %2) does not work with the waywallen engine %3, so live \
         wallpapers are not shown."
            .to_string(),
        "Update the %1 package.".to_string(),
        "A copy of the Plasma wallpaper plugin for waywallen in your home folder (from the KDE Store \
         or installed by hand) hides the %1 package. Rexon left the copy as is. If live wallpapers do \
         not appear, use the packaged version — the copy is kept as a backup."
            .to_string(),
        "Rexon is installing the %1 package for live wallpapers. It is built from source, which can \
         take several minutes."
            .to_string(),
        "Stage: %1".to_string(),
        "Live wallpapers need the %1 package. Install it, and the wallpapers will appear.".to_string(),
        "Live wallpapers need the %1 package, and Rexon could not install it: %2".to_string(),
        w::package_unsupported_reason(),
    ];
    for lang in rexon_core::i18n::parse_linguas(rexon_core::i18n::LINGUAS) {
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

/// The X11 output host client finds the output element in the same package
/// Rexon ships for Plasma: the bundled one first, the installed one as a fallback.
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
    // The bundled package of the developer tree: the wrapper and the module are in place.
    let tree = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data");
    let base = w::display_package_candidates(Some(tree), "")[0].clone();
    if let Some(so) = w::find_runtime_so(&base, Some(w::RUNTIME_SO_REL)) {
        let qml = w::display_item_qml_of(&so).unwrap();
        assert!(std::path::Path::new(&qml).is_file(), "{qml}");
    }
}

/// A grey «Apply» without output for scenes — with an explanation (GNOME
/// without the waywallen extension, Cinnamon Wayland without layer-shell, an
/// unfamiliar compositor); where Rexon or the engine brings up the display —
/// the former words.
#[test]
fn no_displays_hint_explains_missing_output() {
    use rexon_core::wallpaper_backend::{DisplaySetup, WallpaperBackend as B};
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    assert!(!w::no_displays_hint_in(DisplaySetup::None, B::ShellExtension).is_empty());
    assert!(!w::no_displays_hint_in(DisplaySetup::None, B::Unsupported).is_empty());
    assert!(w::no_displays_hint_in(DisplaySetup::None, B::DaemonLayerShell).is_empty(), "дисплей даёт движок");
    assert_eq!(
        w::no_displays_hint_in(DisplaySetup::RexonHost, B::ShellExtension),
        w::no_displays_hint(DisplaySetup::RexonHost),
        "Cinnamon с layer-shell: вывод поднимает Rexon"
    );
}

/// X11 output host client: the packaged `Waywallen.Display` module first, the
/// plasmoid wrappers as fallbacks (owner's decision of 25.11.2026: the output on
/// XFCE/MATE must not disappear when the package carries no bundle).
#[test]
fn display_item_prefers_packaged_module_with_fallback() {
    let dirs = w::qml_import_dirs("/opt/qml:relative", "/usr/lib/qt6/qml/");
    assert_eq!(dirs[0], "/opt/qml");
    assert_eq!(dirs[1], "/usr/lib/qt6/qml", "env first, no duplicates");
    assert!(dirs.contains(&"/usr/lib64/qt6/qml".to_string()));
    assert_eq!(dirs.iter().filter(|d| *d == "/usr/lib/qt6/qml").count(), 1);

    let wrapper = w::system_display_wrapper_path("/run/user/1000", "/home/u");
    assert_eq!(wrapper.as_deref(), Some("/run/user/1000/rexon-waywallen-display.qml"));
    assert_eq!(
        w::system_display_wrapper_path("", "/home/u").as_deref(),
        Some("/home/u/.cache/rexon/rexon-waywallen-display.qml")
    );
    assert_eq!(w::system_display_wrapper_path("", ""), None);
    assert!(w::SYSTEM_DISPLAY_WRAPPER_QML.contains("import Waywallen.Display as WW"));

    let bundled = "/usr/lib/rexon/rexon/kpackage/org.waywallen.kde/contents/ui/Plugin/PluginDisplay.qml".to_string();
    let plasmoids = vec![bundled.clone()];
    let module_qmldir = "/usr/lib/qt6/qml/Waywallen/Display/qmldir";
    let with_module = |p: &str| p == module_qmldir || p == bundled;
    let without_module = |p: &str| p == bundled;
    assert_eq!(
        w::pick_display_item(&dirs, wrapper.as_deref(), &plasmoids, &with_module),
        Some(w::DisplayItemSource::SystemModule { wrapper: wrapper.clone().unwrap() })
    );
    assert_eq!(
        w::pick_display_item(&dirs, wrapper.as_deref(), &plasmoids, &without_module),
        Some(w::DisplayItemSource::Plasmoid { qml: bundled.clone() }),
        "no packaged module: the bundled plasmoid"
    );
    assert_eq!(
        w::pick_display_item(&dirs, None, &plasmoids, &with_module).map(|s| s.qml().to_string()),
        Some(bundled.clone()),
        "nowhere to write the wrapper: the plasmoid"
    );
    assert_eq!(w::pick_display_item(&dirs, wrapper.as_deref(), &[], &|_| false), None);
}

/// Packaged plugin: where it is looked for and how pacman's answer is read.
#[test]
fn system_install_dirs_follow_xdg_data_dirs() {
    assert_eq!(
        w::system_install_dirs("", "/home/u"),
        vec![
            "/usr/local/share/plasma/wallpapers/org.waywallen.kde".to_string(),
            "/usr/share/plasma/wallpapers/org.waywallen.kde".to_string(),
        ],
        "empty XDG_DATA_DIRS: the specification's default"
    );
    assert_eq!(
        w::system_install_dirs("/home/u/.local/share:/opt/kde/share/:relative:/opt/kde/share", "/home/u"),
        vec!["/opt/kde/share/plasma/wallpapers/org.waywallen.kde".to_string()],
        "the user's own copy is not the package; relative and repeated entries are skipped"
    );
}

#[test]
fn pacman_owner_is_parsed() {
    let file = "/usr/share/plasma/wallpapers/org.waywallen.kde/metadata.json";
    assert_eq!(w::pacman_owner_argv("/usr/bin/pacman", file), vec!["/usr/bin/pacman", "-Qo", file]);
    assert_eq!(
        w::parse_pacman_owner(&format!("{file} is owned by plasma-wallpaper-waywallen 0.4.6-1\n")),
        Some(("plasma-wallpaper-waywallen".to_string(), "0.4.6".to_string()))
    );
    assert_eq!(
        w::parse_pacman_owner(&format!("{file} is owned by plasma-wallpaper-waywallen-git 1:0.4.6.r3.g1234abc-2")),
        Some(("plasma-wallpaper-waywallen-git".to_string(), "0.4.6.r3.g1234abc".to_string())),
        "epoch and pkgrel are stripped"
    );
    assert_eq!(w::parse_pacman_owner(&format!("error: No package owns {file}")), None);
    assert_eq!(w::parse_pacman_owner(""), None);
    assert_eq!(w::parse_pacman_owner(&format!("{file} is owned by lonely")), None, "no version");
    assert_eq!(w::upstream_version("0.4.6"), Some("0.4.6".to_string()));
    assert_eq!(w::upstream_version(" "), None);
}
