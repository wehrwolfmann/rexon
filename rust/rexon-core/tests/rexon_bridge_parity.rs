//! Golden check of the bridge port against the original `rexon/rexon_bridge.py`.
//!
//! The golden `golden/rexon_bridge_cases.json` was captured from live Python:
//! `datetime.now`/`time.time` frozen (2026-07-22T12:00:00.123456 /
//! 1753185600.0), paths → tempdir; only the command file names
//! and the JSON contents get into the golden, without absolute paths. No daemon/sockets/network
//! were used — the bridge is entirely file-based.
//!
//! Cases of deliberate deviations (fixes of the original's bugs) are checked not
//! against the golden but by separate marked tests — the Python behaviour there
//! was a crash or data loss.

mod i18n_pin;

use rexon_core::rexon_bridge::{
    all_de_reactions_value, app_running, build_command, build_event, de_reactions_value,
    de_switch_updates, event_reactions, get_app_state, get_bonzi_mood, get_bonzi_position,
    get_current_de, get_current_icons, get_current_theme, get_de_display, get_extensions_count,
    get_installed_des, get_last_switch, get_rexon_version, is_rexon_installed,
    is_video_wallpaper_active, lookup_de_reactions, merge_update, read_bridge, write_bridge,
    RexonBridge, DE_REACTIONS, EVENT_REACTIONS, MAX_CMD_SIZE,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const NOW_ISO: &str = "2026-07-22T12:00:00.123456";
const EPOCH_MS: u64 = 1_753_185_600_000;
const EPOCH: f64 = 1_753_185_600.0;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/rexon_bridge_cases.json"))
        .expect("golden парсится")
}

/// A fresh bridge in a unique temporary directory.
fn fresh_bridge(tag: &str) -> RexonBridge {
    let dir = std::env::temp_dir().join(format!(
        "rexon_bridge_parity_{}_{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let b = RexonBridge::new(dir.join("bridge.json"), dir.join("bridge_commands"));
    b.ensure_dirs().expect("каталоги создаются");
    b
}

/// A state snapshot: the contents of bridge.json + the command files (name → text),
/// like `snap()` in the python golden capture script.
fn snap(b: &RexonBridge) -> (Option<String>, BTreeMap<String, String>) {
    let bridge = std::fs::read_to_string(&b.bridge_file).ok();
    let mut cmds = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(&b.commands_dir) {
        for e in entries.filter_map(|e| e.ok()) {
            if e.path().is_file() {
                let name = e.file_name().into_string().unwrap();
                cmds.insert(name, std::fs::read_to_string(e.path()).unwrap());
            }
        }
    }
    (bridge, cmds)
}

/// The golden snapshot `{"bridge": …, "commands": {…}}` in the same shape.
fn golden_snap(v: &Value) -> (Option<String>, BTreeMap<String, String>) {
    let bridge = v["bridge"].as_str().map(str::to_string);
    let cmds = v["commands"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, val)| (k.clone(), val.as_str().unwrap().to_string()))
        .collect();
    (bridge, cmds)
}

// ============================================================================
// Statics: reactions
// ============================================================================

/// The full dictionary of DE reactions matches Python's `DE_REACTIONS`
/// (the set of DEs, the order and the contents of all lines).
#[test]
fn all_de_reactions_match_golden() {
    // The golden holds the original Russian lines: they come from po/ru.po now.
    let _lang = i18n_pin::ru();
    let g = golden();
    assert_eq!(all_de_reactions_value(), g["all_de_reactions"]);
    // The DE order as in the original.
    let keys: Vec<&str> = DE_REACTIONS.iter().map(|(k, _)| *k).collect();
    let golden_keys: Vec<&str> = g["all_de_reactions"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, golden_keys);
}

/// The full dictionary of event reactions matches Python's `EVENT_REACTIONS`.
#[test]
fn event_reactions_match_golden() {
    // The golden holds the original Russian lines: they come from po/ru.po now.
    let _lang = i18n_pin::ru();
    let g = golden();
    let mut built = Map::new();
    for (k, _) in EVENT_REACTIONS {
        built.insert(
            (*k).to_string(),
            Value::Array(event_reactions(k).into_iter().map(Value::from).collect()),
        );
    }
    assert_eq!(Value::Object(built), g["event_reactions_full"]);
}

/// `get_de_reactions`: an exact match, normalization "cinnamon-mint" →
/// "cinnamon", unknown/empty → `{}` — all inputs from the golden.
#[test]
fn de_reactions_lookup_matches_golden() {
    // The golden holds the original Russian lines: they come from po/ru.po now.
    let _lang = i18n_pin::ru();
    let g = golden();
    for (input, expected) in g["de_reactions_lookup"].as_object().unwrap() {
        assert_eq!(
            de_reactions_value(input),
            *expected,
            "get_de_reactions({input:?})"
        );
    }
    // Typed access agrees with the JSON variant.
    assert!(lookup_de_reactions("kde-x11").is_some());
    assert!(lookup_de_reactions("nope").is_none());
}

/// `get_event_reactions`: known types, the unknown `de_switched` → [].
#[test]
fn event_reactions_lookup_matches_golden() {
    // The golden holds the original Russian lines: they come from po/ru.po now.
    let _lang = i18n_pin::ru();
    let g = golden();
    for (input, expected) in g["event_reactions_lookup"].as_object().unwrap() {
        let got: Vec<Value> = event_reactions(input).into_iter().map(Value::from).collect();
        assert_eq!(Value::Array(got), *expected, "get_event_reactions({input:?})");
    }
}

// ============================================================================
// Scenario A: writing Rexon's state (byte-for-byte with json.dump(indent=2))
// ============================================================================

/// The sequence update_rexon_state → notify_de_switch → all notify →
/// update_app_state gives byte-wise the same bridge.json and command/event files
/// as Python (key order, indent=2, ensure_ascii=False, file names).
#[test]
fn rexon_writes_scenario_byte_identical() {
    let g = golden();
    let steps = g["scenario_rexon_writes"].as_array().unwrap();
    let b = fresh_bridge("writes");

    let mut updates = Map::new();
    updates.insert("current_de".into(), Value::from("kde"));
    updates.insert("current_de_display".into(), Value::from("KDE Plasma 6"));
    updates.insert(
        "installed_des".into(),
        serde_json::json!(["kde", "gnome"]),
    );
    updates.insert("current_theme".into(), Value::from("Dracula"));
    updates.insert("current_icons".into(), Value::from("Papirus-Dark"));
    updates.insert("rexon_version".into(), Value::from("0.0.1"));
    updates.insert("extensions_count".into(), Value::from(3));
    b.update_rexon_state(updates, NOW_ISO).unwrap();
    assert_eq!(snap(&b), golden_snap(&steps[0]), "update_rexon_state");

    b.notify_de_switch("kde", "gnome", NOW_ISO, EPOCH_MS).unwrap();
    assert_eq!(snap(&b), golden_snap(&steps[1]), "notify_de_switch");

    b.notify_theme_changed("Nord", NOW_ISO, EPOCH_MS).unwrap();
    b.notify_de_installed("mate", NOW_ISO, EPOCH_MS).unwrap();
    b.notify_de_removed("xfce", NOW_ISO, EPOCH_MS).unwrap();
    b.notify_extension_installed("clock-widget", NOW_ISO, EPOCH_MS).unwrap();
    b.notify_backup_created(NOW_ISO, EPOCH_MS).unwrap();
    b.notify_video_wallpaper(true, "Ночной город", true, NOW_ISO, EPOCH_MS).unwrap();
    b.notify_video_wallpaper(false, "", true, NOW_ISO, EPOCH_MS).unwrap();
    assert_eq!(snap(&b), golden_snap(&steps[2]), "after_all_notifies");

    let mut app = Map::new();
    app.insert("running".into(), Value::Bool(true));
    app.insert("pid".into(), Value::from(12345));
    app.insert("mood".into(), Value::from("happy"));
    app.insert("position".into(), serde_json::json!({"x": 10, "y": 20}));
    b.update_app_state("bonzi", app, NOW_ISO).unwrap();
    assert_eq!(snap(&b), golden_snap(&steps[3]), "update_app_state");
}

/// Live wallpapers: starting and stopping have THEIR OWN events, the scene name goes
/// as a field on start, and the `video_wallpaper_active` flag is written in any
/// case — even when the notice is suppressed.
///
/// ⚠️ Not golden: the original did not have this behaviour at all. Stopping only
/// cleared the flag and sent NOTHING, the event did not carry the scene name, and
/// the notice could not be suppressed — there is nothing to compare with.
#[test]
fn video_wallpaper_events_carry_name_and_stop() {
    let b = fresh_bridge("wallpaper_events");

    // Start with a name: the `name` field on the event.
    b.notify_video_wallpaper(true, "Ночной город", true, NOW_ISO, EPOCH_MS)
        .unwrap();
    assert!(is_video_wallpaper_active(&b.read()), "признак поднят");
    let (_, cmds) = snap(&b);
    let started: Value =
        serde_json::from_str(&cmds[&format!("event_video_wallpaper_started_{EPOCH_MS}.json")])
            .unwrap();
    assert_eq!(started["params"]["type"], "video_wallpaper_started");
    assert_eq!(started["params"]["name"], "Ночной город");

    // Stop: its own event, without a name.
    b.notify_video_wallpaper(false, "", true, NOW_ISO, EPOCH_MS).unwrap();
    assert!(!is_video_wallpaper_active(&b.read()), "признак погашен");
    let (_, cmds) = snap(&b);
    let stopped: Value =
        serde_json::from_str(&cmds[&format!("event_video_wallpaper_stopped_{EPOCH_MS}.json")])
            .unwrap();
    assert_eq!(stopped["params"]["type"], "video_wallpaper_stopped");
    assert_eq!(
        stopped["params"].as_object().unwrap().len(),
        1,
        "у остановки нет полей, кроме типа"
    );

    // An empty name and a whitespace-only name are equivalent to no name.
    for name in ["", "   "] {
        let b = fresh_bridge("wallpaper_noname");
        b.notify_video_wallpaper(true, name, true, NOW_ISO, EPOCH_MS).unwrap();
        let (_, cmds) = snap(&b);
        let ev: Value =
            serde_json::from_str(&cmds[&format!("event_video_wallpaper_started_{EPOCH_MS}.json")])
                .unwrap();
        assert_eq!(
            ev["params"].as_object().unwrap().len(),
            1,
            "название {name:?} поля давать не должно"
        );
    }

    // A suppressed notice: the flag changes, no events appear at all.
    let b = fresh_bridge("wallpaper_silent");
    b.notify_video_wallpaper(true, "Ночной город", false, NOW_ISO, EPOCH_MS)
        .unwrap();
    assert!(is_video_wallpaper_active(&b.read()), "признак поднят и молча");
    let (_, cmds) = snap(&b);
    assert!(cmds.is_empty(), "подмена обоев событий не шлёт: {cmds:?}");
    b.notify_video_wallpaper(false, "", false, NOW_ISO, EPOCH_MS).unwrap();
    assert!(!is_video_wallpaper_active(&b.read()), "признак погашен и молча");
    assert!(snap(&b).1.is_empty(), "остановка под замену тоже молчит");
}

// ============================================================================
// Scenario B: getters
// ============================================================================

/// All getters on a filled bridge.json give Python's values
/// (including normalization of current_de "cinnamon-mint" in get_de_reactions).
#[test]
fn getters_filled_match_golden() {
    // The golden holds the original Russian lines: they come from po/ru.po now.
    let _lang = i18n_pin::ru();
    let g = golden();
    let exp = &g["getters_filled"];
    let b = fresh_bridge("getters");
    let prepared: Value = serde_json::json!({
        "rexon": {
            "rexon_installed": true,
            "rexon_version": "0.0.1",
            "current_de": "cinnamon-mint",
            "current_de_display": "Cinnamon 6.0",
            "installed_des": ["kde", "cinnamon-mint"],
            "current_theme": "Dracula",
            "current_icons": "Papirus",
            "extensions_count": 7,
            "video_wallpaper_active": true,
            "switched_from": "kde",
            "switched_to": "cinnamon-mint",
            "last_de_switch": "2026-07-21T10:00:00",
        },
        "app_bonzi": {
            "running": true,
            "mood": "happy",
            "position": {"x": 5, "y": 7},
        },
    });
    write_bridge(&b.bridge_file, prepared.as_object().unwrap()).unwrap();
    let data = b.read();

    assert_eq!(is_rexon_installed(&data), exp["is_rexon_installed"].as_bool().unwrap());
    assert_eq!(get_rexon_version(&data), exp["get_rexon_version"].as_str().unwrap());
    assert_eq!(get_current_de(&data), exp["get_current_de"].as_str().unwrap());
    assert_eq!(get_de_display(&data), exp["get_de_display"].as_str().unwrap());
    let des: Vec<Value> = get_installed_des(&data).into_iter().map(Value::from).collect();
    assert_eq!(Value::Array(des), exp["get_installed_des"]);
    assert_eq!(get_current_theme(&data), exp["get_current_theme"].as_str().unwrap());
    assert_eq!(get_current_icons(&data), exp["get_current_icons"].as_str().unwrap());
    assert_eq!(get_extensions_count(&data), exp["get_extensions_count"].as_i64().unwrap());
    assert_eq!(
        is_video_wallpaper_active(&data),
        exp["is_video_wallpaper_active"].as_bool().unwrap()
    );
    assert_eq!(get_last_switch(&data).unwrap(), exp["get_last_switch"]);
    assert_eq!(
        Value::Object(get_app_state(&data, "bonzi")),
        exp["get_app_state_bonzi"]
    );
    assert_eq!(get_bonzi_mood(&data), exp["get_bonzi_mood"].as_str().unwrap());
    assert_eq!(get_bonzi_position(&data), exp["get_bonzi_position"]);
    // get_de_reactions() without an argument = reactions for current_de from the file.
    assert_eq!(
        de_reactions_value(&get_current_de(&data)),
        exp["get_de_reactions_current"]
    );
}

/// Getters on a missing bridge.json — Python's defaults
/// ("unknown"/""/0/false/None/"neutral"/{"x":0,"y":0}).
#[test]
fn getters_empty_match_golden() {
    let g = golden();
    let exp = &g["getters_empty"];
    let data: Map<String, Value> = Map::new();

    assert_eq!(is_rexon_installed(&data), exp["is_rexon_installed"].as_bool().unwrap());
    assert_eq!(get_rexon_version(&data), exp["get_rexon_version"].as_str().unwrap());
    assert_eq!(get_current_de(&data), exp["get_current_de"].as_str().unwrap());
    assert_eq!(get_de_display(&data), exp["get_de_display"].as_str().unwrap());
    assert!(get_installed_des(&data).is_empty());
    assert_eq!(get_current_theme(&data), exp["get_current_theme"].as_str().unwrap());
    assert_eq!(get_current_icons(&data), exp["get_current_icons"].as_str().unwrap());
    assert_eq!(get_extensions_count(&data), exp["get_extensions_count"].as_i64().unwrap());
    assert_eq!(
        is_video_wallpaper_active(&data),
        exp["is_video_wallpaper_active"].as_bool().unwrap()
    );
    assert!(exp["get_last_switch"].is_null());
    assert!(get_last_switch(&data).is_none());
    assert!(get_app_state(&data, "bonzi").is_empty());
    assert_eq!(get_bonzi_mood(&data), exp["get_bonzi_mood"].as_str().unwrap());
    assert_eq!(get_bonzi_position(&data), exp["get_bonzi_position"]);
}

/// A broken bridge.json → `_read()` returns `{}` (Python logs a warning).
#[test]
fn read_broken_bridge_is_empty() {
    let g = golden();
    assert_eq!(g["read_broken_bridge"], serde_json::json!({}));
    let b = fresh_bridge("broken");
    std::fs::write(&b.bridge_file, "{broken json").unwrap();
    assert!(b.read().is_empty());
}

// ============================================================================
// Scenario C: commands / events / cleanup
// ============================================================================

/// Set a file's mtime (the analogue of `os.utime` in the python generator).
fn set_mtime(path: &Path, epoch: f64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(epoch);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// The full scenario: send_command/send_event → poll (with and without delete) →
/// removing garbage (broken JSON, JSON that is not an object, oversize; `.txt` is not
/// touched) → cleanup_commands by age → reset. All snapshots are
/// byte-wise as in Python; the poll lists are the same objects in the same order.
#[test]
fn commands_scenario_matches_golden() {
    let g = golden();
    let c = &g["scenario_commands"];
    let b = fresh_bridge("commands");

    let mut p = Map::new();
    p.insert("text".into(), Value::from("Привет!"));
    b.send_command("bonzi", "say", Some(p), NOW_ISO, EPOCH_MS).unwrap();
    let mut p = Map::new();
    p.insert("de".into(), Value::from("kde"));
    b.send_command("rexon", "switch_de", Some(p), NOW_ISO, EPOCH_MS).unwrap();
    b.send_command("bonzi", "animate", None, NOW_ISO, EPOCH_MS).unwrap();
    let mut d = Map::new();
    d.insert("theme_name".into(), Value::from("Nord"));
    b.send_event("theme_changed", d, NOW_ISO, EPOCH_MS).unwrap();
    assert_eq!(snap(&b), golden_snap(&c["after_send"]), "after_send");

    let polled = b.poll_commands("bonzi", false);
    assert_eq!(Value::Array(polled), c["poll_keep"], "poll_keep");
    assert_eq!(snap(&b), golden_snap(&c["after_poll_keep"]), "after_poll_keep");

    let polled = b.poll_commands("bonzi", true);
    assert_eq!(Value::Array(polled), c["poll_delete"], "poll_delete");
    assert_eq!(snap(&b), golden_snap(&c["after_poll_delete"]), "after_poll_delete");

    let events = b.poll_events(true);
    assert_eq!(Value::Array(events), c["poll_events"], "poll_events");
    assert_eq!(snap(&b), golden_snap(&c["after_poll_events"]), "after_poll_events");

    // Garbage: broken JSON, JSON that is not an object, oversize, not .json.
    std::fs::write(b.commands_dir.join("aaa_broken.json"), "{{{").unwrap();
    std::fs::write(b.commands_dir.join("list.json"), "[1, 2]").unwrap();
    std::fs::write(
        b.commands_dir.join("big.json"),
        "x".repeat((MAX_CMD_SIZE + 10) as usize),
    )
    .unwrap();
    std::fs::write(b.commands_dir.join("notes.txt"), "hi").unwrap();
    let polled = b.poll_commands("bonzi", false);
    assert_eq!(Value::Array(polled), c["poll_garbage"], "poll_garbage");
    assert_eq!(snap(&b), golden_snap(&c["after_poll_garbage"]), "after_poll_garbage");

    // Cleanup by age: the old one (age 400 > 300) is removed, the fresh one and
    // notes.txt (age 10) stay; a file with the real mtime (in the future
    // relative to EPOCH) has a negative age and stays.
    b.send_command("bonzi", "old", Some(Map::new()), NOW_ISO, EPOCH_MS).unwrap();
    set_mtime(
        &b.commands_dir.join("bonzi_old_1753185600000.json"),
        EPOCH - 400.0,
    );
    b.send_command("bonzi", "fresh", Some(Map::new()), NOW_ISO, EPOCH_MS).unwrap();
    set_mtime(
        &b.commands_dir.join("bonzi_fresh_1753185600000.json"),
        EPOCH - 10.0,
    );
    set_mtime(&b.commands_dir.join("notes.txt"), EPOCH - 10.0);
    b.cleanup_commands(300.0, EPOCH);
    assert_eq!(snap(&b), golden_snap(&c["after_cleanup"]), "after_cleanup");

    b.reset().unwrap();
    assert_eq!(snap(&b), golden_snap(&c["after_reset"]), "after_reset");
}

// ============================================================================
// Builders (pure) — consistency with the files from the golden
// ============================================================================

/// `build_command`/`build_event` without writing give the same JSON as the files
/// in the golden scenario (key order: target, command, params, timestamp, id).
#[test]
fn builders_match_golden_files() {
    let g = golden();
    let files = g["scenario_commands"]["after_send"]["commands"]
        .as_object()
        .unwrap();

    let mut p = Map::new();
    p.insert("text".into(), Value::from("Привет!"));
    let msg = build_command("bonzi", "say", Some(p), NOW_ISO, EPOCH_MS, 0);
    assert_eq!(msg.filename, "bonzi_say_1753185600000.json");
    assert_eq!(msg.id, "bonzi_say_1753185600000");
    let expected: Value =
        serde_json::from_str(files["bonzi_say_1753185600000.json"].as_str().unwrap()).unwrap();
    assert_eq!(msg.value, expected);

    // params=None → {} (like `params or {}`).
    let msg = build_command("bonzi", "animate", None, NOW_ISO, EPOCH_MS, 0);
    let expected: Value =
        serde_json::from_str(files["bonzi_animate_1753185600000.json"].as_str().unwrap()).unwrap();
    assert_eq!(msg.value, expected);

    let mut d = Map::new();
    d.insert("theme_name".into(), Value::from("Nord"));
    let msg = build_event("theme_changed", d, NOW_ISO, EPOCH_MS, 0);
    assert_eq!(msg.filename, "event_theme_changed_1753185600000.json");
    let expected: Value =
        serde_json::from_str(files["event_theme_changed_1753185600000.json"].as_str().unwrap())
            .unwrap();
    assert_eq!(msg.value, expected);
}

/// Section updates on a DE switch — the same 4 keys in the same order.
#[test]
fn de_switch_updates_keys() {
    let m = de_switch_updates("kde", "gnome", NOW_ISO);
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["current_de", "last_de_switch", "switched_from", "switched_to"]
    );
    assert_eq!(m["current_de"], "gnome");
    assert_eq!(m["switched_from"], "kde");
    assert_eq!(m["last_de_switch"], NOW_ISO);
}

// ============================================================================
// Deliberate deviations (fixes of the original's bugs — the golden is for the fixed
// behaviour, the Python behaviour was a crash/data loss)
// ============================================================================

/// Fix: a section that is NOT a dict in bridge.json (garbage from a third-party application)
/// is silently replaced by a dict. In the original `data[section].update(...)`
/// raised AttributeError both in the main branch and in the fallback — any notify
/// crashed, the bridge stayed broken forever.
#[test]
fn fixed_merge_into_non_dict_section() {
    let mut data = Map::new();
    data.insert("rexon".into(), Value::from("мусор"));
    let mut updates = Map::new();
    updates.insert("current_de".into(), Value::from("kde"));
    merge_update(&mut data, "rexon", updates, NOW_ISO);
    let rexon = data["rexon"].as_object().unwrap();
    assert_eq!(rexon["current_de"], "kde");
    assert_eq!(rexon["_updated_at"], NOW_ISO);
}

/// Fix: a non-integer pid (a string) in `is_app_running`. In the original
/// `os.kill("123", 0)` raised TypeError past `except (ProcessLookupError,
/// PermissionError)`. Now — like the branch without a pid: truthy(running).
#[test]
fn fixed_app_running_non_int_pid() {
    let mut state = Map::new();
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from("123"));
    assert!(app_running(&state, |_| unreachable!("pid_alive не должен вызываться")));
}

/// Fix: pid ≤ 0. In the original `os.kill(-1, 0)` sent a signal to a process GROUP
/// (kill's special semantics) and almost always gave true. Now
/// such a pid is ignored, liveness is decided by truthy(running).
#[test]
fn fixed_app_running_non_positive_pid() {
    let mut state = Map::new();
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from(-1));
    assert!(app_running(&state, |_| unreachable!("pid_alive не должен вызываться")));
    // pid=0 is falsy in Python too (`if pid:`) — the branch without a process check.
    state.insert("pid".into(), Value::from(0));
    assert!(app_running(&state, |_| unreachable!()));
}

/// The ordinary branches of `is_app_running` (parity): running falsy → false;
/// a valid pid → the pid_alive answer.
#[test]
fn app_running_normal_branches() {
    let mut state = Map::new();
    assert!(!app_running(&state, |_| true)); // running is absent
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from(4242));
    assert!(app_running(&state, |pid| {
        assert_eq!(pid, 4242);
        true
    }));
    assert!(!app_running(&state, |_| false)); // the process is dead
    state.insert("running".into(), Value::Bool(false));
    assert!(!app_running(&state, |_| true)); // running=false — the pid is not checked
}

/// Fix: a name collision with two commands in the same millisecond.
/// In the original the second file silently overwrote the first one (the command was lost) —
/// now the second one gets the `_1` suffix, both commands get through.
#[test]
fn fixed_command_name_collision_gets_suffix() {
    let b = fresh_bridge("collision");
    let f1 = b
        .send_command("bonzi", "say", Some(Map::new()), NOW_ISO, EPOCH_MS)
        .unwrap();
    let f2 = b
        .send_command("bonzi", "say", Some(Map::new()), NOW_ISO, EPOCH_MS)
        .unwrap();
    assert_eq!(f1, "bonzi_say_1753185600000.json");
    assert_eq!(f2, "bonzi_say_1753185600000_1.json");
    // The id inside the file agrees with the name.
    let cmds = b.poll_commands("bonzi", false);
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[1]["id"], "bonzi_say_1753185600000_1");
}

/// Default paths are built from $HOME (like expanduser in the original).
#[test]
fn default_paths_under_home() {
    let b = RexonBridge::new_default();
    assert!(b.bridge_file.ends_with(".config/rexon/bridge.json"));
    assert!(b.commands_dir.ends_with(".config/rexon/bridge_commands"));
}

/// Writing bridge.json is atomic: after a write there is no `.tmp` stub, and
/// reading returns what was written.
#[test]
fn write_bridge_atomic_roundtrip() {
    let b = fresh_bridge("atomic");
    let mut data = Map::new();
    data.insert("rexon".into(), serde_json::json!({"current_de": "kde"}));
    write_bridge(&b.bridge_file, &data).unwrap();
    let tmp = PathBuf::from(format!("{}.tmp", b.bridge_file.display()));
    assert!(!tmp.exists());
    assert_eq!(read_bridge(&b.bridge_file), data);
}

/// Rule #12: EVERY supported desktop must have lines.
///
/// The incident this test closes: LXQt, niri and sway were added to
/// `_DE_DATABASE` (rexon/system.py), but forgotten in `DE_REACTIONS` — BonziBuddy
/// was silent in these three environments (`get_de_reactions` returned `{}`, and
/// the "name up to the hyphen" normalization does not help them, their names have no hyphen).
/// Adding a DE to the database — add the lines too, otherwise the test will fail.
#[test]
fn every_supported_de_has_reactions() {
    let missing: Vec<&str> = rexon_core::system::list_de_ids()
        .into_iter()
        .filter(|de| lookup_de_reactions(de).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "нет реплик BonziBuddy для DE: {missing:?} — правило #12"
    );
    // The "name up to the hyphen" normalization covers editions like cinnamon-mint.
    assert!(lookup_de_reactions("cinnamon-mint").is_some());
}
