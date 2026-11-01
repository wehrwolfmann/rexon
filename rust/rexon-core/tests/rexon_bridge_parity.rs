//! Golden-сверка порта моста с оригиналом `rexon/rexon_bridge.py`.
//!
//! Эталон `golden/rexon_bridge_cases.json` снят с живого Python:
//! `datetime.now`/`time.time` заморожены (2026-07-22T12:00:00.123456 /
//! 1753185600.0), пути → tempdir; в golden попадают только имена файлов
//! команд и содержимое JSON, без абсолютных путей. Демон/сокеты/сеть не
//! использовались — мост целиком файловый.
//!
//! Кейсы намеренных расхождений (починки багов оригинала) сверяются не
//! по golden, а отдельными тестами с пометкой — Python-поведение там
//! было крэшем или потерей данных.

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

/// Свежий мост в уникальном временном каталоге.
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

/// Снимок состояния: содержимое bridge.json + файлы команд (имя → текст),
/// как `snap()` в python-скрипте снятия эталона.
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

/// Golden-снимок `{"bridge": …, "commands": {…}}` в ту же форму.
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
// Статика: реакции
// ============================================================================

/// Полный словарь DE-реакций совпадает с Python `DE_REACTIONS`
/// (состав DE, порядок и содержимое всех реплик).
#[test]
fn all_de_reactions_match_golden() {
    let g = golden();
    assert_eq!(all_de_reactions_value(), g["all_de_reactions"]);
    // Порядок DE как в оригинале.
    let keys: Vec<&str> = DE_REACTIONS.iter().map(|(k, _)| *k).collect();
    let golden_keys: Vec<&str> = g["all_de_reactions"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, golden_keys);
}

/// Полный словарь событийных реакций совпадает с Python `EVENT_REACTIONS`.
#[test]
fn event_reactions_match_golden() {
    let g = golden();
    let mut built = Map::new();
    for (k, list) in EVENT_REACTIONS {
        built.insert(
            (*k).to_string(),
            Value::Array(list.iter().map(|s| Value::from(*s)).collect()),
        );
    }
    assert_eq!(Value::Object(built), g["event_reactions_full"]);
}

/// `get_de_reactions`: точное совпадение, нормализация "cinnamon-mint" →
/// "cinnamon", неизвестные/пустые → `{}` — все входы из golden.
#[test]
fn de_reactions_lookup_matches_golden() {
    let g = golden();
    for (input, expected) in g["de_reactions_lookup"].as_object().unwrap() {
        assert_eq!(
            de_reactions_value(input),
            *expected,
            "get_de_reactions({input:?})"
        );
    }
    // Типизированный доступ согласован с JSON-вариантом.
    assert!(lookup_de_reactions("kde-x11").is_some());
    assert!(lookup_de_reactions("nope").is_none());
}

/// `get_event_reactions`: известные типы, неизвестный `de_switched` → [].
#[test]
fn event_reactions_lookup_matches_golden() {
    let g = golden();
    for (input, expected) in g["event_reactions_lookup"].as_object().unwrap() {
        let got: Vec<Value> = event_reactions(input)
            .iter()
            .map(|s| Value::from(*s))
            .collect();
        assert_eq!(Value::Array(got), *expected, "get_event_reactions({input:?})");
    }
}

// ============================================================================
// Сценарий A: запись состояния Rexon (байт-в-байт с json.dump(indent=2))
// ============================================================================

/// Последовательность update_rexon_state → notify_de_switch → все notify →
/// update_app_state даёт побайтно те же bridge.json и файлы команд/событий,
/// что Python (порядок ключей, indent=2, ensure_ascii=False, имена файлов).
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

/// Живые обои: у запуска и остановки СВОИ события, название сцены идёт
/// полем при запуске, а признак `video_wallpaper_active` пишется в любом
/// случае — даже когда весть подавлена.
///
/// ⚠️ Не golden: у оригинала этого поведения не было вовсе. Остановка только
/// гасила признак и не слала НИЧЕГО, названия сцены событие не носило, а
/// подавить весть было нельзя — сверять не с чем.
#[test]
fn video_wallpaper_events_carry_name_and_stop() {
    let b = fresh_bridge("wallpaper_events");

    // Запуск с названием: поле `name` при событии.
    b.notify_video_wallpaper(true, "Ночной город", true, NOW_ISO, EPOCH_MS)
        .unwrap();
    assert!(is_video_wallpaper_active(&b.read()), "признак поднят");
    let (_, cmds) = snap(&b);
    let started: Value =
        serde_json::from_str(&cmds[&format!("event_video_wallpaper_started_{EPOCH_MS}.json")])
            .unwrap();
    assert_eq!(started["params"]["type"], "video_wallpaper_started");
    assert_eq!(started["params"]["name"], "Ночной город");

    // Остановка: своё событие, названия при нём нет.
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

    // Пустое название и название из одних пробелов равносильны отсутствию.
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

    // Подавленная весть: признак меняется, событий не появляется вовсе.
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
// Сценарий B: геттеры
// ============================================================================

/// Все геттеры на заполненном bridge.json дают значения Python
/// (включая нормализацию current_de "cinnamon-mint" в get_de_reactions).
#[test]
fn getters_filled_match_golden() {
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
    // get_de_reactions() без аргумента = реакции для current_de из файла.
    assert_eq!(
        de_reactions_value(&get_current_de(&data)),
        exp["get_de_reactions_current"]
    );
}

/// Геттеры на отсутствующем bridge.json — дефолты Python
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

/// Битый bridge.json → `_read()` возвращает `{}` (Python логирует warning).
#[test]
fn read_broken_bridge_is_empty() {
    let g = golden();
    assert_eq!(g["read_broken_bridge"], serde_json::json!({}));
    let b = fresh_bridge("broken");
    std::fs::write(&b.bridge_file, "{broken json").unwrap();
    assert!(b.read().is_empty());
}

// ============================================================================
// Сценарий C: команды / события / очистка
// ============================================================================

/// Выставить mtime файла (аналог `os.utime` в python-генераторе).
fn set_mtime(path: &Path, epoch: f64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(epoch);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// Полный сценарий: send_command/send_event → poll (с delete и без) →
/// удаление мусора (битый JSON, JSON-не-объект, oversize; `.txt` не
/// трогается) → cleanup_commands по возрасту → reset. Все снимки —
/// побайтно как Python; списки poll — те же объекты в том же порядке.
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

    // Мусор: битый JSON, JSON-не-объект, oversize, не-.json.
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

    // Очистка по возрасту: старый (age 400 > 300) удаляется, свежий и
    // notes.txt (age 10) остаются; файл с реальным mtime (будущее
    // относительно EPOCH) — отрицательный age, остаётся.
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
// Билдеры (чистые) — согласованность с файлами из golden
// ============================================================================

/// `build_command`/`build_event` без записи дают тот же JSON, что файлы
/// в golden-сценарии (порядок ключей: target, command, params, timestamp, id).
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

    // params=None → {} (как `params or {}`).
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

/// Обновления секции при переключении DE — те же 4 ключа в том же порядке.
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
// Намеренные расхождения (починки багов оригинала — golden под починенное
// поведение, Python-поведение было крэшем/потерей данных)
// ============================================================================

/// Починка: секция-НЕ-словарь в bridge.json (мусор стороннего приложения)
/// молча заменяется словарём. В оригинале `data[section].update(...)`
/// ронял AttributeError и в основной ветке, и в fallback — любой notify
/// падал, мост оставался сломанным навсегда.
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

/// Починка: нецелый pid (строка) в `is_app_running`. В оригинале
/// `os.kill("123", 0)` ронял TypeError мимо `except (ProcessLookupError,
/// PermissionError)`. Теперь — как ветка без pid: truthy(running).
#[test]
fn fixed_app_running_non_int_pid() {
    let mut state = Map::new();
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from("123"));
    assert!(app_running(&state, |_| unreachable!("pid_alive не должен вызываться")));
}

/// Починка: pid ≤ 0. В оригинале `os.kill(-1, 0)` слал сигнал ГРУППЕ
/// процессов (спецсемантика kill) и почти всегда давал true. Теперь
/// такой pid игнорируется, живость решается по truthy(running).
#[test]
fn fixed_app_running_non_positive_pid() {
    let mut state = Map::new();
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from(-1));
    assert!(app_running(&state, |_| unreachable!("pid_alive не должен вызываться")));
    // pid=0 falsy и в Python (`if pid:`) — ветка без проверки процесса.
    state.insert("pid".into(), Value::from(0));
    assert!(app_running(&state, |_| unreachable!()));
}

/// Обычные ветки `is_app_running` (паритет): running falsy → false;
/// валидный pid → ответ pid_alive.
#[test]
fn app_running_normal_branches() {
    let mut state = Map::new();
    assert!(!app_running(&state, |_| true)); // running отсутствует
    state.insert("running".into(), Value::Bool(true));
    state.insert("pid".into(), Value::from(4242));
    assert!(app_running(&state, |pid| {
        assert_eq!(pid, 4242);
        true
    }));
    assert!(!app_running(&state, |_| false)); // процесс мёртв
    state.insert("running".into(), Value::Bool(false));
    assert!(!app_running(&state, |_| true)); // running=false — pid не проверяется
}

/// Починка: коллизия имён при двух командах в одну миллисекунду.
/// В оригинале второй файл молча затирал первый (команда терялась) —
/// теперь второй получает суффикс `_1`, обе команды доходят.
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
    // id внутри файла согласован с именем.
    let cmds = b.poll_commands("bonzi", false);
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[1]["id"], "bonzi_say_1753185600000_1");
}

/// Пути по умолчанию строятся от $HOME (как expanduser в оригинале).
#[test]
fn default_paths_under_home() {
    let b = RexonBridge::new_default();
    assert!(b.bridge_file.ends_with(".config/rexon/bridge.json"));
    assert!(b.commands_dir.ends_with(".config/rexon/bridge_commands"));
}

/// Запись bridge.json атомарна: после write нет `.tmp`-огрызка, а
/// чтение возвращает записанное.
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

/// Правило #12: у КАЖДОГО поддерживаемого рабочего стола должны быть реплики.
///
/// Инцидент, который этот тест закрывает: LXQt, niri и sway добавили в
/// `_DE_DATABASE` (rexon/system.py), а в `DE_REACTIONS` забыли — BonziBuddy
/// в этих трёх окружениях молчал (`get_de_reactions` отдавал `{}`, а
/// нормализация «имя до дефиса» им не помогает, дефиса в именах нет).
/// Добавляешь DE в базу — добавь и реплики, иначе тест упадёт.
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
    // Нормализация «имя до дефиса» покрывает редакции вроде cinnamon-mint.
    assert!(lookup_de_reactions("cinnamon-mint").is_some());
}
