//! Golden-сверка порта `wallpaper_mmap.rs` с оригиналом
//! `rexon/wallpaper_mmap.py`. ⚠️ Сам модуль снят с дерева 2026-08-23:
//! записанные ниже наблюдения — единственная оставшаяся запись его
//! поведения, перегенерировать их больше нечем.
//!
//! Эталон снят read-only с ЖИВОГО `rexon.wallpaper_mmap` одноразовым
//! генератором: реальный xprop НЕ запускался (subprocess подменён стабом с
//! захватом argv), Qt не стартовал (PySide6 подменён стабами, interval QTimer
//! и имя приложения захвачены), `prime_output_file` писал только во
//! временный каталог (сняты байты/размер/права). Покрыто: разбор ENV-размера
//! экрана, путь вывода из ENV, layout кадра (размер + чёрная непрозрачная
//! строка прайминга), детект Wayland-сессии, argv обеих xprop-команд,
//! интервал headless-тика.
//!
//! `golden/wallpaper_mmap_cases.json` — снятые значения.

use rexon_core::wallpaper_mmap as wm;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/wallpaper_mmap_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn screen_size_from_env() {
    let g = fx();
    for case in g["env_screen_size"].as_array().unwrap() {
        let w = case["width"].as_str();
        let h = case["height"].as_str();
        let expected = (
            case["result"][0].as_i64().unwrap(),
            case["result"][1].as_i64().unwrap(),
        );
        assert_eq!(
            wm::env_screen_size(w, h),
            expected,
            "env_screen_size {}",
            s(&case["label"])
        );
    }
}

#[test]
fn output_path_from_env() {
    let g = fx();
    for case in g["env_output_path"].as_array().unwrap() {
        let raw = case["value"].as_str();
        let expected = case["result"].as_str().map(String::from);
        assert_eq!(
            wm::env_output_path(raw),
            expected,
            "env_output_path {}",
            s(&case["label"])
        );
    }
}

#[test]
fn wayland_detect() {
    let g = fx();
    for case in g["is_wayland_session"].as_array().unwrap() {
        // ENV не задана → пустая строка (os.environ.get(..., ""))
        let st = case["session_type"].as_str().unwrap_or("");
        let wd = case["wayland_display"].as_str().unwrap_or("");
        assert_eq!(
            wm::is_wayland_session(st, wd),
            case["result"].as_bool().unwrap(),
            "is_wayland_session {}",
            s(&case["label"])
        );
    }
}

/// Набор EWMH у окна обоев тот же, что ставил Python через `xprop`: тип
/// рабочего стола и состояния. Поменялся способ (свойства до показа,
/// просьбы корневому окну после), а не то, чем окно становится. Одно
/// намеренное расхождение — без `_NET_WM_STATE_BELOW`: xfwm4 по нему
/// поднимает окно из слоя стола над окном значков
/// ([`wm::X11_DESKTOP_STATES`]).
#[test]
fn x11_desktop_plan_keeps_python_atoms() {
    let g = fx();
    let plan = wm::x11_desktop_plan();
    let atoms_of = |name: &str| -> Vec<String> {
        let p = plan
            .before_map
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("до показа не ставится {name}"));
        assert_eq!(p.kind, wm::XA_ATOM, "{name} — атомы");
        p.values
            .iter()
            .map(|v| match v {
                wm::X11Word::Atom(a) => a.to_string(),
                wm::X11Word::Num(n) => panic!("{name}: число {n} вместо атома"),
            })
            .collect()
    };
    for case in g["xprop"].as_array().unwrap() {
        let calls: Vec<Vec<String>> = case["calls"].as_array().unwrap().iter().map(str_vec).collect();
        // argv Python: xprop -id N -f PROP 32a -set PROP VALUE[,VALUE…]
        for call in &calls {
            let (prop, value) = (&call[4], &call[8]);
            let python: Vec<String> = value
                .split(',')
                .filter(|a| *a != "_NET_WM_STATE_BELOW")
                .map(str::to_string)
                .collect();
            assert_eq!(atoms_of(prop), python, "{prop} разошёлся с Python");
        }
    }
}

/// Всё ставится ДО показа: тип первым (WM читает его при показе), состояние
/// и «на всех столах» = 0xFFFFFFFF. Показанному окну `_NET_WM_STATE`
/// свойством больше не пишется — только просьбой.
#[test]
fn x11_desktop_plan_sets_everything_before_map() {
    let plan = wm::x11_desktop_plan();
    let names: Vec<&str> = plan.before_map.iter().map(|p| p.name).collect();
    assert_eq!(names, ["_NET_WM_WINDOW_TYPE", "_NET_WM_STATE", "_NET_WM_DESKTOP"]);
    let desktop = &plan.before_map[2];
    assert_eq!(desktop.kind, wm::XA_CARDINAL);
    assert_eq!(desktop.values, [wm::X11Word::Num(0xFFFF_FFFF)]);
    assert!(
        plan.after_map_properties.iter().all(|p| p.name != "_NET_WM_STATE"),
        "состояние показанного окна снова пишется свойством — WM его не перечитает"
    );
}

/// Просьбы `_NET_WM_STATE` по EWMH: data.l[0] = ADD, l[1] — атом, l[2] —
/// атом или 0, l[3] = 1 (приложение); вместе покрывают все состояния ровно
/// по разу. `_NET_WM_DESKTOP`: l[0] = 0xFFFFFFFF, l[1] = 1.
#[test]
fn x11_desktop_messages_follow_ewmh() {
    let plan = wm::x11_desktop_plan();
    let mut covered: Vec<&str> = Vec::new();
    for m in plan.after_map_messages.iter().filter(|m| m.message_type == "_NET_WM_STATE") {
        assert_eq!(m.data[0], wm::X11Word::Num(wm::NET_WM_STATE_ADD));
        assert_eq!(m.data[3], wm::X11Word::Num(1), "источник просьбы — приложение");
        match m.data[1] {
            wm::X11Word::Atom(a) => covered.push(a),
            other => panic!("в l[1] не атом: {other:?}"),
        }
        match m.data[2] {
            wm::X11Word::Atom(a) => covered.push(a),
            wm::X11Word::Num(0) => {}
            other => panic!("в l[2] не атом и не 0: {other:?}"),
        }
    }
    covered.sort_unstable();
    let mut want = wm::X11_DESKTOP_STATES.to_vec();
    want.sort_unstable();
    assert_eq!(covered, want);
    let desk = plan
        .after_map_messages
        .iter()
        .find(|m| m.message_type == "_NET_WM_DESKTOP")
        .expect("нет просьбы _NET_WM_DESKTOP");
    assert_eq!(desk.data[0], wm::X11Word::Num(0xFFFF_FFFF));
    assert_eq!(desk.data[1], wm::X11Word::Num(1));
}

/// Окну типа «рабочий стол» BELOW не ставится ни до показа, ни просьбой:
/// xfwm4 4.20 переносит такое окно из слоя стола (0) в слой BELOW (2) —
/// над окном значков, и опустить его оттуда нельзя. Marco и Muffin слой
/// окна стола от BELOW не меняют, им он не нужен.
#[test]
fn x11_desktop_plan_never_sets_below() {
    let plan = wm::x11_desktop_plan();
    let below = wm::X11Word::Atom("_NET_WM_STATE_BELOW");
    for p in plan.before_map.iter().chain(&plan.after_map_properties) {
        assert!(!p.values.contains(&below), "{} несёт BELOW", p.name);
    }
    for m in &plan.after_map_messages {
        assert!(!m.data.contains(&below), "просьба {} несёт BELOW", m.message_type);
    }
    assert!(!wm::x11_desktop_plan_atoms(&plan).contains(&"_NET_WM_STATE_BELOW"));
}

/// Атомы узнаются у сервера один раз каждый; в списке есть всё, что
/// упоминает план.
#[test]
fn x11_desktop_plan_atoms_cover_plan() {
    let plan = wm::x11_desktop_plan();
    let atoms = wm::x11_desktop_plan_atoms(&plan);
    let mut uniq = atoms.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq.len(), atoms.len(), "атом спрашивается дважды");
    let lookup = |name: &str| atoms.iter().position(|a| *a == name).map(|i| i as u32 + 100);
    for p in plan.before_map.iter().chain(&plan.after_map_properties) {
        assert!(lookup(p.name).is_some(), "{}", p.name);
        assert!(wm::x11_resolve_words(&p.values, lookup).is_some(), "{}", p.name);
    }
    for m in &plan.after_map_messages {
        assert!(lookup(m.message_type).is_some(), "{}", m.message_type);
        assert!(wm::x11_resolve_words(&m.data, lookup).is_some(), "{}", m.message_type);
    }
    // Неизвестный серверу атом — слово не ставится вовсе.
    assert_eq!(wm::x11_resolve_words(&[wm::X11Word::Atom("X")], |_| None), None);
}

/// Раскладка 32 байт ClientMessage для `xcb_send_event`.
#[test]
fn x11_client_message_layout() {
    let ev = wm::x11_client_message_bytes(0x0420_0007, 301, [1, 302, 303, 1, 0]);
    assert_eq!(ev[0], 33, "код ClientMessage");
    assert_eq!(ev[1], 32, "формат 32");
    assert_eq!(&ev[2..4], &[0, 0], "номер ставит сервер");
    let word = |i: usize| u32::from_ne_bytes(ev[i..i + 4].try_into().unwrap());
    assert_eq!(word(4), 0x0420_0007, "окно");
    assert_eq!(word(8), 301, "message_type");
    assert_eq!([word(12), word(16), word(20), word(24), word(28)], [1, 302, 303, 1, 0]);
    assert_eq!(
        wm::ROOT_MESSAGE_EVENT_MASK,
        (1 << 20) | (1 << 19),
        "SubstructureRedirect | SubstructureNotify"
    );
}

#[test]
fn prime_frame_layout() {
    let g = fx();
    for case in g["prime_output_file"].as_array().unwrap() {
        let label = s(&case["label"]);
        let w = case["width"].as_i64().unwrap();
        let h = case["height"].as_i64().unwrap();
        assert_eq!(
            wm::frame_size(w, h),
            case["size"].as_u64().unwrap(),
            "frame_size {label}"
        );
        // Файл прайминга = чёрная непрозрачная строка × max(1, height)
        if let Some(hex) = case.get("hex").and_then(Value::as_str) {
            let row = wm::black_row(w);
            let mut expected_file = Vec::new();
            for _ in 0..h.max(1) {
                expected_file.extend_from_slice(&row);
            }
            let file_hex: String = expected_file.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(file_hex, hex, "prime bytes {label}");
        }
        if let Some(mode) = case.get("mode_octal").and_then(Value::as_str) {
            assert_eq!(format!("0o{:o}", wm::FRAME_FILE_MODE), mode, "mode {label}");
        }
    }
}

#[test]
fn frame_path() {
    // Канонический путь extension'ов: GLib.get_user_runtime_dir() + имя файла
    assert_eq!(
        wm::default_frame_path("/run/user/1000"),
        "/run/user/1000/rexon-wp-frame.rgba"
    );
}

#[test]
fn headless_tick() {
    let g = fx();
    for case in g["headless_tick"].as_array().unwrap() {
        let fps = case["fps"].as_i64().unwrap();
        assert_eq!(
            wm::headless_tick_ms(fps),
            case["interval_ms"].as_i64().unwrap(),
            "headless_tick_ms fps={fps}"
        );
    }
    assert_eq!(wm::HEADLESS_APP_NAME, s(&g["headless_app_name"]));
}

/// План прайминга собирает ровно то, что оригинал делал в `prime_output_file`
/// руками: размер файла, число записей строки (`max(1, height)`), саму строку
/// и права. Эталон — тот же golden, что у `prime_frame_layout`.
#[test]
fn prime_plan_matches_manual_layout() {
    let g = fx();
    for case in g["prime_output_file"].as_array().unwrap() {
        let label = s(&case["label"]);
        let w = case["width"].as_i64().unwrap();
        let h = case["height"].as_i64().unwrap();
        let plan = wm::prime_plan(w, h);
        assert_eq!(plan.size, case["size"].as_u64().unwrap(), "size {label}");
        assert_eq!(plan.rows, h.max(1) as u64, "rows {label}");
        assert_eq!(plan.row, wm::black_row(w), "row {label}");
        assert_eq!(plan.width, w.max(1), "width {label}");
        assert_eq!(plan.height, h.max(1), "height {label}");
        assert_eq!(plan.mode, wm::FRAME_FILE_MODE, "mode {label}");
        // Файл ровно покрывается строками: щелей и хвоста быть не должно,
        // иначе расширение оболочки прочитает мусор за последним пикселем.
        assert_eq!(
            plan.rows * plan.row.len() as u64,
            plan.size,
            "строки не покрывают файл {label}"
        );
    }
}

/// `REXON_WP_FPS` — порт `_env_fps` из `video_wallpaper.py`: мусор и выход за
/// 1..=120 откатываются к 30, чтобы кривая переменная не оставила человека
/// без обоев.
#[test]
fn env_fps_falls_back_on_garbage() {
    assert_eq!(wm::env_fps(None), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("   ")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("60fps")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("24.5")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("0")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("121")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some("-1")), wm::DEFAULT_FPS);
    assert_eq!(wm::env_fps(Some(" 60 ")), 60);
    assert_eq!(wm::env_fps(Some("1")), wm::MIN_FPS);
    assert_eq!(wm::env_fps(Some("120")), wm::MAX_FPS);
}

/// Признак сеанса Wayland — ОДИН на весь Rexon: здесь только переадресация
/// в `wallpaper_backend`, своей реализации быть не должно.
#[test]
fn wayland_detect_shares_engine_with_backend() {
    for (st, wd) in [
        ("wayland", ""),
        ("Wayland", ""),
        (" wayland ", ""),
        ("x11", ""),
        ("x11", "wayland-1"),
        ("", " "),
        ("", ""),
        ("tty", ""),
    ] {
        assert_eq!(
            wm::is_wayland_session(st, wd),
            rexon_core::wallpaper_backend::is_wayland_session(st, wd),
            "детект разошёлся: {st:?}/{wd:?}"
        );
    }
}

/// Имя Qt-приложения headless-цикла не должно разъезжаться с общей таблицей
/// точек входа.
#[test]
fn headless_app_name_matches_entry_table() {
    let ident = rexon_core::qt_app::identity("rexon.wallpaper_mmap")
        .expect("точки входа rexon.wallpaper_mmap нет в таблице qt_app");
    assert_eq!(ident.application_name, wm::HEADLESS_APP_NAME);
}

/// Сторож от возврата вспышки чужих обоев при смене обоев.
///
/// Что стерегём. Файл кадра `rexon-wp-frame.rgba` один на сеанс, а расширения
/// GNOME и Cinnamon решают «кадр годен» по одному признаку — размеру файла
/// (`bytes.get_size() < expected` → тик пропускается). Пока поток кадров при
/// старте открывал ПРЕЖНИЙ файл и сразу раздувал его до `width*height*4`,
/// расширение видело полный размер и рисовало последний кадр ПРЕЖНИХ обоев,
/// пока новый хост не успевал записать свой первый (замер на подставном пути:
/// 34 мс на готовом окне плюс весь запуск процесса хоста; а пока прежний хост
/// доживал после `kill`, оба писали в один файл и картинки чередовались
/// секундами).
///
/// Проверка идёт по исходнику шима: сам поток кадров без Qt-окна не поднять,
/// а признак починки в нём короткий и однозначный — файл отцепляется и до
/// первого настоящего кадра остаётся коротким.
#[test]
fn frame_file_is_detached_and_not_preallocated() {
    let cpp_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/cpp/rexon_wallpaper_host.cpp"
    );
    let raw = std::fs::read_to_string(cpp_path).expect("нет шима rexon_wallpaper_host.cpp");
    // Смотрим на КОД, а не на пояснения: слова `ftruncate` и `unlink` стоят и
    // в комментариях рядом с починкой, и по ним проверка сказала бы неправду.
    let cpp: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let unlink = cpp
        .find("::unlink(")
        .expect("поток кадров больше не отцепляет прежний файл — кадры умирающего \
                 хоста снова попадут в обои, пришедшие ему на смену");
    let open = cpp
        .find("::open(")
        .expect("в шиме не стало открытия файла кадра — проверка смотрит не туда");
    assert!(
        unlink < open,
        "прежний файл отцепляется ПОСЛЕ открытия — новый поток пишет в чужой файл"
    );
    assert!(
        cpp.contains("O_TRUNC"),
        "файл кадра открывается без O_TRUNC: если отцепить не удалось, в нём \
         останется готовый кадр прежних обоев"
    );
    assert!(
        !cpp.contains("ftruncate"),
        "файл кадра снова раздувается заранее: полный размер = «кадр готов» для \
         расширений оболочки, и до первого своего кадра они покажут чужой"
    );
}

// ── Сторожа по исходникам расширений оболочки ─────────────────────────────

/// Пути обоих `extension.js`: GNOME и Cinnamon.
///
/// Файлы разные (ESM против классических `imports`), а дефекты у них были
/// общие — оба лечатся одинаково, поэтому и стерегутся вместе.
fn shell_extension_sources() -> [(&'static str, String); 2] {
    let gnome = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/gnome-extension/rexon-wallpaper@rexon.app/extension.js"
    );
    let cinnamon = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/cinnamon-extension/rexon-wallpaper@rexon.app/extension.js"
    );
    [
        ("GNOME", std::fs::read_to_string(gnome).expect("нет extension.js GNOME")),
        (
            "Cinnamon",
            std::fs::read_to_string(cinnamon).expect("нет extension.js Cinnamon"),
        ),
    ]
}

/// Вырезать общий блок диагностики запуска между метками.
fn launch_diagnostics_block(src: &str, de: &str) -> String {
    let begin = "[rexon-wp:launch-diagnostics:begin]";
    let end = "[rexon-wp:launch-diagnostics:end]";
    let from = src
        .find(begin)
        .unwrap_or_else(|| panic!("в extension.js {de} нет метки начала общего блока"))
        + begin.len();
    let to = src[from..]
        .find(end)
        .unwrap_or_else(|| panic!("в extension.js {de} нет метки конца общего блока"))
        + from;
    src[from..to].to_string()
}

/// Сторож: объяснение отказа в обоих расширениях — ОДНО И ТО ЖЕ.
///
/// Что стерегём. Дефект «расширение зовёт не тот интерпретатор и молчит о
/// причине» был одинаков в обоих файлах, и лечение у него одно (правило
/// #12). Разъехаться эти блоки могут незаметно: правку вносят в тот файл,
/// на котором сидит человек, а второй проверить не на чем — GNOME и
/// Cinnamon рядом не поставишь. Поэтому сравниваем посимвольно.
#[test]
fn shell_extensions_share_launch_diagnostics() {
    let [(de_a, src_a), (de_b, src_b)] = shell_extension_sources();
    let a = launch_diagnostics_block(&src_a, de_a);
    let b = launch_diagnostics_block(&src_b, de_b);
    assert!(
        !a.trim().is_empty(),
        "общий блок диагностики пуст — сторожить нечего"
    );
    assert_eq!(
        a, b,
        "блоки диагностики запуска в расширениях {de_a} и {de_b} разошлись: \
         правку внесли в один файл, а второй остался с прежним объяснением"
    );
}

/// Вырезать общий блок чтения кадра между метками.
fn frame_read_block(src: &str, de: &str) -> String {
    let begin = "[rexon-wp:frame-read:begin]";
    let end = "[rexon-wp:frame-read:end]";
    let from = src
        .find(begin)
        .unwrap_or_else(|| panic!("в extension.js {de} нет метки начала блока чтения кадра"))
        + begin.len();
    let to = src[from..]
        .find(end)
        .unwrap_or_else(|| panic!("в extension.js {de} нет метки конца блока чтения кадра"))
        + from;
    src[from..to].to_string()
}

/// Сторож: кадр оба расширения читают ОДНИМ кодом, огороженным счётчиком
/// поколения, и сам тик файл кадра напрямую не отображает.
///
/// Что стерегём. Хост пишет кадр под `flock(LOCK_EX)`, а GJS замок взять не
/// может; без счётчика читатель попадал на середину записи (замер: 12 рваных
/// кадров из 180 тиков на 1920×1080/30). Окончание имени счётчика в JS
/// обязано совпадать с ядром — иначе расширение молча читало бы
/// несуществующий файл и вернулось к рваным кадрам.
#[test]
fn shell_extensions_share_frame_read() {
    let [(de_a, src_a), (de_b, src_b)] = shell_extension_sources();
    let a = frame_read_block(&src_a, de_a);
    let b = frame_read_block(&src_b, de_b);
    assert_eq!(
        a, b,
        "блоки чтения кадра в расширениях {de_a} и {de_b} разошлись"
    );
    assert!(
        a.contains(&format!("const FRAME_GEN_SUFFIX = '{}';", wm::FRAME_GEN_SUFFIX)),
        "окончание имени счётчика поколения в extension.js не совпадает с ядром"
    );
    for (de, src) in [(de_a, &src_a), (de_b, &src_b)] {
        let block = frame_read_block(src, de);
        let outside = src.replace(&block, "");
        let code: String = outside
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("MappedFile"),
            "extension.js {de} снова отображает файл кадра мимо readWholeFrame — \
             без сверки счётчика поколения кадры рвутся"
        );
        assert!(
            code.contains("readWholeFrame("),
            "extension.js {de} не читает кадр через readWholeFrame"
        );
    }
}

/// Договор счётчика поколения: путь рядом с файлом кадра, u64 little-endian
/// (JS читает `DataView.getBigUint64(0, true)`), и шим пишет нечётное ДО строк
/// кадра и чётное ПОСЛЕ.
#[test]
fn frame_generation_contract() {
    assert_eq!(
        wm::frame_generation_path("/run/user/1000/rexon-wp-frame.rgba"),
        "/run/user/1000/rexon-wp-frame.rgba.gen"
    );
    assert_eq!(wm::FRAME_GEN_BYTES, 8);
    assert_eq!(wm::frame_generation_bytes(0x0102_0304_0506_0708), [8, 7, 6, 5, 4, 3, 2, 1]);

    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/cpp/rexon_wallpaper_host.cpp"
    ))
    .expect("нет шима rexon_wallpaper_host.cpp");
    let body_from = raw.find("void write_frame(").expect("в шиме нет write_frame");
    let body = &raw[body_from..];
    let rows = body.find("::write(stream.fd").expect("write_frame не пишет строки кадра");
    let first_gen = body
        .find("write_generation(stream, stream.generation)")
        .expect("write_frame не отмечает поколение перед записью кадра");
    let last_gen = body[rows..]
        .find("write_generation(stream, stream.generation)")
        .expect("write_frame не отмечает поколение после записи кадра");
    assert!(first_gen < rows, "нечётное поколение ставится не ДО строк кадра");
    assert!(last_gen > 0, "чётное поколение ставится не ПОСЛЕ строк кадра");
}

/// Платформа Qt хоста в режиме потока кадров. Видео и Flash — всегда
/// `offscreen`, даже если оболочка унаследовала хосту `QT_QPA_PLATFORM=wayland`:
/// иначе окно обоев выходит обычным окном поверх рабочего стола (замер:
/// `xdg_toplevel` «RexonVideoWallpaper» под Wayland-композитором без
/// layer-shell).
#[test]
fn host_qpa_platform_own_frames_forced_offscreen_when_streaming() {
    use wm::StreamCapture::OwnFrames;
    for (wl, x) in [("", ""), ("wayland-0", ""), ("", ":0"), ("wayland-0", ":0")] {
        assert_eq!(wm::host_qpa_platform(true, OwnFrames, None, wl, x), Some("offscreen"));
        assert_eq!(wm::host_qpa_platform(true, OwnFrames, Some(""), wl, x), Some("offscreen"));
        assert_eq!(wm::host_qpa_platform(true, OwnFrames, Some("wayland"), wl, x), Some("offscreen"));
        assert_eq!(wm::host_qpa_platform(true, OwnFrames, Some("xcb"), wl, x), Some("offscreen"));
        assert_eq!(wm::host_qpa_platform(true, OwnFrames, Some("offscreen"), wl, x), None);
        assert_eq!(wm::host_qpa_platform(false, OwnFrames, None, wl, x), None);
        assert_eq!(wm::host_qpa_platform(false, OwnFrames, Some("wayland"), wl, x), None);
    }
}

/// Веб в режиме потока рисует `QQuickRenderControl` без окна — и получает
/// видеокарту только на НАСТОЯЩЕЙ платформе сеанса: на `offscreen` у Qt нет
/// EGL, а QtWebEngine там не кладёт GPU-кадр в сцену (замер: WebGL нет, CSS
/// 20 к/с при 166 % CPU). Порядок: Wayland, затем X11/XWayland, без обоих —
/// `offscreen`. Унаследованное значение перебивается и здесь.
#[test]
fn host_qpa_platform_render_control_takes_session_platform() {
    use wm::StreamCapture::RenderControl;
    assert_eq!(wm::host_qpa_platform(true, RenderControl, None, "wayland-0", ":0"), Some("wayland"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, Some("offscreen"), "wayland-0", ""), Some("wayland"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, Some("xcb"), "wayland-0", ":0"), Some("wayland"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, Some("wayland"), "wayland-0", ""), None);
    assert_eq!(wm::host_qpa_platform(true, RenderControl, None, "", ":0"), Some("xcb"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, Some("wayland"), "  ", ":1"), Some("xcb"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, None, "", ""), Some("offscreen"));
    assert_eq!(wm::host_qpa_platform(true, RenderControl, Some("offscreen"), "", ""), None);
    // Не поток — платформу выбирает Qt сам (слой, X11, отладочное окно).
    assert_eq!(wm::host_qpa_platform(false, RenderControl, None, "wayland-0", ":0"), None);
}

/// Показать окно ради снимка кадра можно только на `offscreen`: на настоящей
/// платформе окно на Mutter выходит поверх рабочего стола.
#[test]
fn stream_window_shows_only_on_offscreen() {
    assert!(wm::stream_window_may_show("offscreen"));
    for platform in ["wayland", "xcb", "", "wayland-egl", "eglfs"] {
        assert!(!wm::stream_window_may_show(platform), "{platform}");
    }
}

/// Таблица «чем снимается кадр»: сцену Qt Quick рисует только веб.
#[test]
fn host_stream_capture_per_mode() {
    use rexon_core::wallpaper_engine::HostMode;
    assert_eq!(wm::host_stream_capture(HostMode::Web), wm::StreamCapture::RenderControl);
    assert_eq!(wm::host_stream_capture(HostMode::Video), wm::StreamCapture::OwnFrames);
    assert_eq!(wm::host_stream_capture(HostMode::Swf), wm::StreamCapture::OwnFrames);
}

/// Сторож: платформу Qt и способ показа все режимы хоста обоев получают
/// ОДНИМ общим решением, а не своей копией. Копия у прежнего веб-хоста уже
/// была — и видео остался без неё: на GNOME/Cinnamon-Wayland его окно
/// выходило поверх рабочего стола. С 15.09.2026 хост один
/// (`rexon-wallpaper-host --mode video|web|swf`): решение живёт в общем
/// `main.rs`, файлы режимов его не повторяют — Flash (`swf.rs`) тоже только
/// планирует, а платформу Qt ему выставляет `main.rs`. Каталог читается
/// целиком — следующий режим попадёт под сторож сам.
#[test]
fn wallpaper_hosts_share_qpa_platform_decision() {
    let gui = format!("{}/../rexon-gui/src/bin", env!("CARGO_MANIFEST_DIR"));
    let dir = format!("{gui}/wallpaper_host");
    let read = |name: &str| -> String {
        let path = format!("{dir}/{name}");
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("нет {path}"));
        raw.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let main = read("main.rs");
    assert!(
        main.contains("prepare_qpa_platform(plan.placement, plan.mode)"),
        "хост не выставляет платформу Qt общим решением"
    );
    assert!(main.contains("host_placement("), "способ показа выбран не общим host_placement");

    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("нет {dir}"))
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    files.sort();
    assert!(files.len() >= 3, "режимы хоста не найдены: {files:?}");
    for name in &files {
        let code = read(name);
        assert!(
            !code.contains("\"QT_QPA_PLATFORM\""),
            "{name}: режим снова решает платформу Qt своей копией"
        );
        if name != "main.rs" {
            assert!(
                !code.contains("host_placement(") && !code.contains("prepare_qpa_platform("),
                "{name}: режим решает способ показа сам, мимо общего main.rs"
            );
        }
        // Chromium поднимается ТОЛЬКО веб-режимом: видео не платит за него.
        assert_eq!(
            code.contains("init_web_engine("),
            name == "web.rs",
            "{name}: подъём QtWebEngine не там, где ему место"
        );
    }
    // Второго хоста обоев рядом больше нет.
    assert!(
        !std::path::Path::new(&format!("{gui}/web_host.rs")).exists(),
        "вернулся отдельный бинарь веб-хоста"
    );
}

/// Сторож: расширения не ищут хост обоев в `PATH` и не зовут Python.
///
/// Что стерегём. `SpawnFlags.SEARCH_PATH` берёт ПЕРВУЮ подходящую программу
/// из `PATH` оболочки, а у покупателя с venv, переносным каталогом или
/// Flatpak это не тот Rexon, что установлен: обои не появлялись, а
/// расширение говорило только «spawn failed». Путь к хосту присылает сам
/// Rexon ключом `daemon-argv` (`wallpaper_engine::SHELL_EXT_DAEMON_KEY`), и
/// второго способа выбрать программу быть не должно.
#[test]
fn shell_extensions_never_search_path_for_host() {
    for (de, raw) in shell_extension_sources() {
        // Смотрим на КОД, а не на пояснения: и `SEARCH_PATH`, и `python3`
        // стоят в комментариях рядом с починкой — по ним проверка сказала бы
        // неправду.
        let code: String = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("SEARCH_PATH"),
            "extension.js {de} снова запускает хост обоев через PATH — \
             у покупателя с venv/portable/Flatpak это чужая программа"
        );
        assert!(
            !code.contains("python"),
            "extension.js {de} снова собирает строку запуска сам: Python-хост \
             снят с дерева, запускать обои нечем, кроме присланного бинаря"
        );
        assert!(
            code.contains(rexon_core::wallpaper_engine::SHELL_EXT_DAEMON_KEY),
            "extension.js {de} перестал читать ключ daemon-argv — строку \
             запуска ему прислать больше некуда"
        );
    }
}

/// Сторож: у каждого отказа запуска есть объяснение и в журнале, и на экране.
///
/// Что стерегём. Прежде расширение молчало ровно там, где человеку нужнее
/// всего: `spawn_async` вернул `false` — не логировалось ничего, хост лёг
/// сразу после старта — в журнал уходил голый код выхода, а на экране
/// оставался чёрный прямоугольник поверх обоев. Наружу не выходило ничего:
/// ни строки в журнал, ни сообщения в оболочке.
#[test]
fn shell_extensions_report_launch_failures_to_user() {
    for (de, raw) in shell_extension_sources() {
        let code: String = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "launchProblem(",     // проверка строки запуска ДО вызова оболочки
            "spawnFailureText(",  // отказ самой оболочки
            "earlyExitText(",     // хост лёг сразу после старта
            "Main.notifyError(",  // сообщение человеку, а не только в журнал
        ] {
            assert!(
                code.contains(needle),
                "extension.js {de} потерял «{needle}»: часть отказов снова \
                 остаётся без объяснения"
            );
        }
    }
}

/// Сторож: расширение Cinnamon ставит кадр туда, где он виден.
///
/// Что стерегём. Расширение искало фон в `Main.layoutManager._bgManagers` —
/// поле GNOME Shell, которого у Cinnamon нет: цикл шёл по пустому массиву, и
/// на Cinnamon Wayland обои не появлялись никогда. Фон там рисует
/// csd-background окнами в `global.bottom_window_group`; кадр встаёт поверх
/// них, а без них — в `Meta.BackgroundGroup`, которую Muffin держит под
/// окнами (любой другой актёр в группе окон всплывает над ними).
#[test]
fn cinnamon_extension_uses_muffin_background_layers() {
    let [_, (de, raw)] = shell_extension_sources();
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("_bgManagers"),
        "extension.js {de} снова ищет фон в _bgManagers — у Cinnamon этого поля нет, \
         обои не появятся"
    );
    for needle in [
        "global.bottom_window_group",   // где живут окна фона csd-background
        "'csd-background'",             // по чему окно фона узнаётся
        "Meta.BackgroundGroup",         // запасное место под окнами
        "'actor-added'",                // csd поднимает окна позже расширения
    ] {
        assert!(
            code.contains(needle),
            "extension.js {de} потерял «{needle}»: кадру снова некуда встать"
        );
    }
}

// ── порядок окна обоев и окна значков стола (X11) ─────────────────────────

fn facts(desktop_type: bool, instance: &str, depth: u8) -> wm::X11ClientFacts {
    wm::X11ClientFacts {
        desktop_type,
        announced_icons: false,
        instance: instance.into(),
        class: instance.into(),
        depth,
    }
}

/// Роль окна: сквозными значки бывают только с альфой И при композиторе.
#[test]
fn x11_icons_window_is_see_through_only_with_alpha_and_compositor() {
    use wm::X11StackRole as R;
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 32), true), R::Icons { see_through: true });
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 32), false), R::Icons { see_through: false });
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 24), true), R::Icons { see_through: false });
    assert_eq!(wm::x11_stack_role(&facts(true, "csd-background", 24), true), R::Background);
    assert_eq!(wm::x11_stack_role(&facts(false, "caja", 32), true), R::Other, "обычное окно Caja");
    // Корневое свойство Caja называет окно стола раньше, чем тип прочитан.
    let mut announced = facts(false, "", 32);
    announced.announced_icons = true;
    assert_eq!(wm::x11_stack_role(&announced, true), R::Icons { see_through: true });
}

/// Дефект 31.10.2026 (MATE): окно обоев показано позже и стоит над
/// непрозрачной Caja — так и надо (иначе обоев не видно); над сквозной —
/// под неё.
#[test]
fn x11_wallpaper_goes_below_see_through_icons_and_above_opaque_ones() {
    use wm::X11Restack as M;
    use wm::X11StackRole as R;
    let caja = 0xa00006;
    let own = 0x3e00007;
    let opaque = [(1, R::Other), (caja, R::Icons { see_through: false }), (own, R::Own)];
    assert_eq!(wm::x11_restack_goal(&opaque, own), None, "непрозрачная Caja: обои остаются сверху");
    let see_through = [(1, R::Other), (caja, R::Icons { see_through: true }), (own, R::Own)];
    assert_eq!(wm::x11_restack_goal(&see_through, own), Some(M::Below(caja)));
    // После перестановки — на месте, второй запрос не нужен.
    let settled = [(own, R::Own), (caja, R::Icons { see_through: true }), (1, R::Other)];
    assert_eq!(wm::x11_restack_goal(&settled, own), None);
    // Caja пересоздала окно непрозрачным (ключ выключили) — обои наверх.
    let reopened = [(own, R::Own), (caja, R::Icons { see_through: false })];
    assert_eq!(wm::x11_restack_goal(&reopened, own), Some(M::Above(caja)));
    // Значков нет — трогать нечего.
    assert_eq!(wm::x11_restack_goal(&[(own, R::Own), (1, R::Other)], own), None);
    // Не наше окно — не наше дело.
    assert_eq!(wm::x11_restack_goal(&see_through, 0x42), None);
}

/// Окно, рисующее только фон (Cinnamon под XWayland), — всегда под обоями;
/// несколько окон значков (xfdesktop по окну на монитор).
#[test]
fn x11_background_painter_stays_below_and_all_icon_windows_count() {
    use wm::X11Restack as M;
    use wm::X11StackRole as R;
    let own = 7;
    let under_bg = [(own, R::Own), (2, R::Background), (3, R::Icons { see_through: true })];
    assert_eq!(wm::x11_restack_goal(&under_bg, own), Some(M::Above(2)));
    let no_icons = [(own, R::Own), (2, R::Background)];
    assert_eq!(wm::x11_restack_goal(&no_icons, own), Some(M::Above(2)));
    let xfdesktop = [
        (3, R::Icons { see_through: false }),
        (own, R::Own),
        (4, R::Icons { see_through: false }),
    ];
    assert_eq!(wm::x11_restack_goal(&xfdesktop, own), Some(M::Above(4)), "над верхним окном значков");
    let nemo = [(3, R::Icons { see_through: true }), (4, R::Icons { see_through: true }), (own, R::Own)];
    assert_eq!(wm::x11_restack_goal(&nemo, own), Some(M::Below(3)), "под нижним окном значков");
    assert_eq!(M::Below(3).sibling_and_mode(), (3, wm::X11_STACK_BELOW));
    assert_eq!(M::Above(4).sibling_and_mode(), (4, wm::X11_STACK_ABOVE));
}

/// Дефект 31.10.2026 (xfwm4 + nemo-desktop): менеджер просьбу о порядке не
/// исполняет, а сверка раз в 2 с просила снова и снова. Теперь — не больше
/// `X11_RESTACK_TRIES` просьб при том же стеке, затем одна запись об отказе и
/// тишина; любое изменение нижнего слоя начинает счёт заново.
#[test]
fn x11_restack_gives_up_once_when_wm_ignores_request() {
    use wm::X11Restack as M;
    use wm::X11RestackStep as S;
    use wm::X11StackRole as R;
    let (nemo, own) = (0x1600004, 0x2200007);
    let stuck = [(nemo, R::Icons { see_through: true }), (1, R::Other), (own, R::Own)];
    let mut t = wm::X11RestackTracker::default();
    for _ in 0..wm::X11_RESTACK_TRIES {
        assert_eq!(t.step(&stuck, own), Some(S::Ask(M::Below(nemo))));
    }
    assert_eq!(t.step(&stuck, own), Some(S::GiveUp(M::Below(nemo))), "отказ — один раз");
    for _ in 0..5 {
        assert_eq!(t.step(&stuck, own), Some(S::Quiet), "дальше молчим");
    }
    // Чужие окна вне нижнего слоя (открыли терминал) счёт не сбрасывают.
    let other_moved = [(nemo, R::Icons { see_through: true }), (own, R::Own), (1, R::Other)];
    assert_eq!(t.step(&other_moved, own), Some(S::Quiet));
    // nemo-desktop пересоздал окно — новая попытка.
    let recreated = [(0x1600009, R::Icons { see_through: true }), (own, R::Own)];
    assert_eq!(t.step(&recreated, own), Some(S::Ask(M::Below(0x1600009))));
    // Менеджер исполнил — счёт обнулён; снова сбилось — снова просим.
    let settled = [(own, R::Own), (0x1600009, R::Icons { see_through: true })];
    assert_eq!(t.step(&settled, own), None);
    assert_eq!(t.step(&recreated, own), Some(S::Ask(M::Below(0x1600009))));
}

#[test]
fn x11_compositor_selection_follows_display_screen() {
    assert_eq!(wm::x11_screen_number(":0"), 0);
    assert_eq!(wm::x11_screen_number(":1.1"), 1);
    assert_eq!(wm::x11_screen_number("localhost:10.2"), 2);
    assert_eq!(wm::x11_screen_number(""), 0);
    assert_eq!(wm::x11_compositor_selection(1), "_NET_WM_CM_S1");
}

/// Дефект 31.10.2026 (xfwm4 + nemo-desktop, показанный раньше хоста):
/// после признанного отказа окно значков пересоздаётся — но только ради
/// цели «под значки» и один раз на окно обоев, без повторов по кругу.
#[test]
fn x11_icons_window_recreated_once_after_give_up() {
    use wm::X11Restack as M;
    use wm::X11StackRole as R;
    let (nemo, own) = (0x400028, 0x60000e);
    let mut opaque = wm::X11RestackTracker::default();
    assert!(!opaque.take_icons_recreate(M::Above(nemo)), "над непрозрачными значками новое окно не поможет");
    let mut t = wm::X11RestackTracker::default();
    assert!(t.take_icons_recreate(M::Below(nemo)));
    assert!(!t.take_icons_recreate(M::Below(nemo)), "второй раз — нет");
    // Новое окно значков снова не под обоями — просим, признаём отказ, но
    // окно значков больше не трогаем.
    let stuck = [(0x400076, R::Icons { see_through: true }), (own, R::Own)];
    for _ in 0..=wm::X11_RESTACK_TRIES {
        t.step(&stuck, own);
    }
    assert!(!t.take_icons_recreate(M::Below(0x400076)));
}
