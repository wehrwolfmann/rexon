//! Golden check of the `wallpaper_mmap.rs` port against the original
//! `rexon/wallpaper_mmap.py`. ⚠️ The module itself was removed from the tree on
//! 2026-08-23: the observations recorded below are the only remaining record
//! of its behaviour, there is nothing left to regenerate them with.
//!
//! The reference was captured read-only from the LIVE `rexon.wallpaper_mmap`
//! by a one-off generator: the real xprop was NOT run (subprocess replaced by
//! a stub capturing argv), Qt was not started (PySide6 replaced by stubs, the
//! QTimer interval and the application name captured), `prime_output_file`
//! wrote only to a temporary directory (bytes/size/permissions captured).
//! Covered: parsing the screen size from ENV, the output path from ENV, the
//! frame layout (size + a black opaque priming row), Wayland session
//! detection, argv of both xprop commands, the headless tick interval.
//!
//! `golden/wallpaper_mmap_cases.json` — the captured values.

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
        // ENV not set → empty string (os.environ.get(..., ""))
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

/// The wallpaper window's EWMH set is the same as Python set via `xprop`: the
/// desktop type and states. The method changed (properties before showing,
/// requests to the root window after), not what the window becomes. One
/// intentional divergence — without `_NET_WM_STATE_BELOW`: xfwm4 uses it to
/// raise the window from the desktop layer above the icon window
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

/// Everything is set BEFORE showing: the type first (the WM reads it on
/// show), the state and «on all desktops» = 0xFFFFFFFF. `_NET_WM_STATE` is no
/// longer written as a property to a shown window — only as a request.
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

/// `_NET_WM_STATE` requests per EWMH: data.l[0] = ADD, l[1] — an atom, l[2] —
/// an atom or 0, l[3] = 1 (application); together they cover all states
/// exactly once. `_NET_WM_DESKTOP`: l[0] = 0xFFFFFFFF, l[1] = 1.
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

/// A window of type «desktop» gets no BELOW, neither before showing nor by
/// request: xfwm4 4.20 moves such a window from the desktop layer (0) to the
/// BELOW layer (2) — above the icon window, and it cannot be lowered from
/// there. Marco and Muffin do not change the desktop window layer on BELOW,
/// they do not need it.
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

/// Atoms are queried from the server once each; the list has everything the
/// plan mentions.
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
    // An atom unknown to the server — the word is not set at all.
    assert_eq!(wm::x11_resolve_words(&[wm::X11Word::Atom("X")], |_| None), None);
}

/// The 32-byte ClientMessage layout for `xcb_send_event`.
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
        // Priming file = a black opaque row × max(1, height)
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
    // The extensions' canonical path: GLib.get_user_runtime_dir() + file name
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

/// The priming plan builds exactly what the original did by hand in
/// `prime_output_file`: file size, the number of row writes (`max(1,
/// height)`), the row itself and the permissions. The reference is the same
/// golden as for `prime_frame_layout`.
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
        // The file is covered by rows exactly: there must be no gaps and no tail,
        // otherwise the shell extension would read garbage after the last pixel.
        assert_eq!(
            plan.rows * plan.row.len() as u64,
            plan.size,
            "строки не покрывают файл {label}"
        );
    }
}

/// `REXON_WP_FPS` is a port of `_env_fps` from `video_wallpaper.py`: garbage
/// and values outside 1..=120 fall back to 30, so that a broken variable does
/// not leave the person without wallpapers.
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

/// The Wayland session sign is ONE for all of Rexon: here only a redirection
/// to `wallpaper_backend`, there must be no implementation of its own.
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

/// The Qt application name of the headless loop must not drift from the
/// shared entry point table.
#[test]
fn headless_app_name_matches_entry_table() {
    let ident = rexon_core::qt_app::identity("rexon.wallpaper_mmap")
        .expect("точки входа rexon.wallpaper_mmap нет в таблице qt_app");
    assert_eq!(ident.application_name, wm::HEADLESS_APP_NAME);
}

/// Guard against the return of a flash of the previous wallpaper on a wallpaper change.
///
/// What we guard. The frame file `rexon-wp-frame.rgba` is one per session, and
/// the GNOME and Cinnamon extensions decide «the frame is valid» by one sign
/// — the file size (`bytes.get_size() < expected` → the tick is skipped).
/// While the frame stream on start opened the FORMER file and immediately
/// grew it to `width*height*4`, the extension saw the full size and drew the
/// last frame of the FORMER wallpaper until the new host managed to write its
/// first one (measured on a mocked path: 34 ms on a ready window plus the
/// whole host process start; and while the former host lingered after
/// `kill`, both wrote into one file and the pictures alternated for seconds).
///
/// The check goes over the shim source: the frame stream itself cannot be
/// brought up without a Qt window, and the sign of the fix in it is short and
/// unambiguous — the file is unlinked and stays short until the first real frame.
#[test]
fn frame_file_is_detached_and_not_preallocated() {
    let cpp_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/cpp/rexon_wallpaper_host.cpp"
    );
    let raw = std::fs::read_to_string(cpp_path).expect("нет шима rexon_wallpaper_host.cpp");
    // We look at the CODE, not at the explanations: the words `ftruncate` and
    // `unlink` also stand in the comments next to the fix, and by them the check
    // would tell a lie.
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

// ── Guards over the shell extension sources ─────────────────────────────

/// Paths of both `extension.js`: GNOME and Cinnamon.
///
/// The files differ (ESM versus classic `imports`), but their defects were
/// shared — both are cured the same way, so they are guarded together.
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

/// Cut out the shared launch diagnostics block between the markers.
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

/// Guard: the refusal explanation in both extensions is THE SAME.
///
/// What we guard. The defect «the extension calls the wrong interpreter and
/// is silent about the reason» was identical in both files, and its cure is
/// one (rule #12). These blocks can drift apart unnoticed: the edit is made
/// in the file the person is sitting on, and the second one cannot be checked
/// — GNOME and Cinnamon cannot be installed side by side. So we compare
/// character by character.
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

/// Guard: both extensions hand a host crash at run time to the Rexon watcher
/// instead of showing their own message on every crash, and a late exit of
/// the former host does not remove the new one's actors.
///
/// What we guard. The crash watcher (`renderer_watch::HostWatch`) brings the
/// host back up by changing the extension settings and tells the person ONCE
/// by itself. An extension complaining on every exit would give a second
/// notification on every crash; and an exit of the former host arriving after
/// the new one came up would erase the new one's actors — the desktop would
/// stay without wallpapers with a live host.
#[test]
fn shell_extensions_leave_crashes_to_rexon() {
    for (de, src) in shell_extension_sources() {
        let block = launch_diagnostics_block(&src, de);
        assert!(block.contains("function isLaunchFailure("), "{de}: в общем блоке нет isLaunchFailure");
        let outside = src.replace(&block, "");
        assert!(
            outside.contains("isLaunchFailure(livedMs)"),
            "{de}: выход хоста показывается сообщением без разбора «отказ запуска / падение на ходу»"
        );
        assert!(
            outside.contains("_daemon_pid !== _p"),
            "{de}: выход прежнего хоста не отличается от выхода текущего"
        );
    }
}

/// Cut out the shared frame-reading block between the markers.
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

/// Guard: both extensions read the frame with ONE piece of code fenced by the
/// generation counter, and the tick itself does not map the frame file directly.
///
/// What we guard. The host writes the frame under `flock(LOCK_EX)`, and GJS
/// cannot take the lock; without the counter the reader landed in the middle
/// of a write (measured: 12 torn frames out of 180 ticks at 1920×1080/30).
/// The counter name suffix in JS must match the core — otherwise the
/// extension would silently read a nonexistent file and go back to torn frames.
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

/// The generation counter contract: a path next to the frame file, u64
/// little-endian (JS reads `DataView.getBigUint64(0, true)`), and the shim
/// writes an odd value BEFORE the frame rows and an even one AFTER.
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

/// The host's Qt platform in frame streaming mode. Video and Flash are always
/// `offscreen`, even if the shell passed `QT_QPA_PLATFORM=wayland` on to the
/// host: otherwise the wallpaper window comes out as a regular window above
/// the desktop (measured: `xdg_toplevel` «RexonVideoWallpaper» under a Wayland
/// compositor without layer-shell).
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

/// Web in streaming mode draws with `QQuickRenderControl` without a window —
/// and gets the GPU only on the REAL session platform: on `offscreen` Qt has
/// no EGL, and QtWebEngine there does not put the GPU frame into the scene
/// (measured: no WebGL, CSS at 20 fps with 166 % CPU). Order: Wayland, then
/// X11/XWayland, without both — `offscreen`. The inherited value is
/// overridden here too.
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
    // Not a stream — Qt chooses the platform itself (layer, X11, debug window).
    assert_eq!(wm::host_qpa_platform(false, RenderControl, None, "wayland-0", ":0"), None);
}

/// Showing the window for a frame grab is allowed only on `offscreen`: on a
/// real platform the window on Mutter comes out above the desktop.
#[test]
fn stream_window_shows_only_on_offscreen() {
    assert!(wm::stream_window_may_show("offscreen"));
    for platform in ["wayland", "xcb", "", "wayland-egl", "eglfs"] {
        assert!(!wm::stream_window_may_show(platform), "{platform}");
    }
}

/// The «what captures the frame» table: only web draws a Qt Quick scene.
#[test]
fn host_stream_capture_per_mode() {
    use rexon_core::wallpaper_engine::HostMode;
    assert_eq!(wm::host_stream_capture(HostMode::Web), wm::StreamCapture::RenderControl);
    assert_eq!(wm::host_stream_capture(HostMode::Video), wm::StreamCapture::OwnFrames);
    assert_eq!(wm::host_stream_capture(HostMode::Swf), wm::StreamCapture::OwnFrames);
}

/// Guard: all wallpaper host modes get the Qt platform and the show method
/// from ONE shared decision, not their own copy. The former web host already
/// had a copy — and video was left without it: on GNOME/Cinnamon Wayland its
/// window came out above the desktop. Since 15.09.2026 there is one host
/// (`rexon-wallpaper-host --mode video|web|swf`): the decision lives in the
/// shared `main.rs`, the mode files do not repeat it — Flash (`swf.rs`) also
/// only plans, and `main.rs` sets the Qt platform for it. The directory is
/// read in full — the next mode falls under the guard by itself.
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
        // Chromium is brought up ONLY by web mode: video does not pay for it.
        assert_eq!(
            code.contains("init_web_engine("),
            name == "web.rs",
            "{name}: подъём QtWebEngine не там, где ему место"
        );
    }
    // There is no second wallpaper host next to it any more.
    assert!(
        !std::path::Path::new(&format!("{gui}/web_host.rs")).exists(),
        "вернулся отдельный бинарь веб-хоста"
    );
}

/// Guard: the extensions do not look for the wallpaper host in `PATH` and do not call Python.
///
/// What we guard. `SpawnFlags.SEARCH_PATH` takes the FIRST matching program
/// from the shell's `PATH`, and for a buyer with a venv, a portable directory
/// or Flatpak that is not the installed Rexon: no wallpapers appeared, and the
/// extension said only «spawn failed». Rexon itself sends the host path in
/// the `daemon-argv` key (`wallpaper_engine::SHELL_EXT_DAEMON_KEY`), and there
/// must be no second way to choose the program.
#[test]
fn shell_extensions_never_search_path_for_host() {
    for (de, raw) in shell_extension_sources() {
        // We look at the CODE, not at the explanations: both `SEARCH_PATH` and
        // `python3` stand in the comments next to the fix — by them the check would
        // tell a lie.
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

/// Guard: every launch refusal has an explanation both in the log and on screen.
///
/// What we guard. Formerly the extension was silent exactly where the person
/// needed it most: `spawn_async` returned `false` — nothing was logged, the
/// host died right after the start — a bare exit code went to the log, and a
/// black rectangle stayed on screen over the wallpaper. Nothing got out:
/// neither a log line nor a message in the shell.
#[test]
fn shell_extensions_report_launch_failures_to_user() {
    for (de, raw) in shell_extension_sources() {
        let code: String = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "launchProblem(",     // checking the launch line BEFORE calling the shell
            "spawnFailureText(",  // the shell's own refusal
            "earlyExitText(",     // the host died right after the start
            "Main.notifyError(",  // a message to the person, not only to the log
        ] {
            assert!(
                code.contains(needle),
                "extension.js {de} потерял «{needle}»: часть отказов снова \
                 остаётся без объяснения"
            );
        }
    }
}

/// Guard: the Cinnamon extension puts the frame where it is visible.
///
/// What we guard. The extension looked for the background in
/// `Main.layoutManager._bgManagers` — a GNOME Shell field Cinnamon does not
/// have: the loop went over an empty array, and on Cinnamon Wayland the
/// wallpapers never appeared. There the background is drawn by csd-background
/// with windows in `global.bottom_window_group`; the frame goes above them,
/// and without them — into `Meta.BackgroundGroup`, which Muffin keeps under
/// the windows (any other actor in the window group floats above them).
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
        "global.bottom_window_group",   // where the csd-background windows live
        "'csd-background'",             // what the background window is recognised by
        "Meta.BackgroundGroup",         // the fallback place under the windows
        "'actor-added'",                // csd brings up its windows later than the extension
    ] {
        assert!(
            code.contains(needle),
            "extension.js {de} потерял «{needle}»: кадру снова некуда встать"
        );
    }
}

/// Guard: on Cinnamon with layer-shell the extension does NOT start the host.
///
/// Live on 15.11.2026 (Cinnamon 6.8 Wayland) next to the Rexon host on the
/// desktop layer ran a second host, brought up by the extension from the old
/// keys: double decoding load. The session sign check must stand in `_reload`
/// BEFORE `_spawnDaemon`, and the sign is the same as the core's without a
/// compositor answer: Wayland and the Cinnamon background service activation
/// file in the data directories.
#[test]
fn cinnamon_extension_skips_host_on_layer_shell() {
    use rexon_core::wallpaper_backend as wb;
    let [_, (de, raw)] = shell_extension_sources();
    let code: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let reload_at = code.find("function _reload()").expect("нет _reload");
    let reload = &code[reload_at..];
    let reload = &reload[..reload.find("\n}\n").expect("конец _reload")];
    let guard = reload.find("if (_layerShellSession())").expect("в _reload нет проверки layer-shell");
    let spawn = reload.find("_spawnDaemon(").expect("в _reload нет запуска хоста");
    let actors = reload.find("_installActors(").expect("в _reload нет актёров");
    assert!(guard < spawn && guard < actors, "extension.js {de}: проверка layer-shell после запуска хоста");
    assert!(
        reload[guard..spawn].contains("return;"),
        "extension.js {de}: проверка layer-shell не прерывает _reload"
    );
    let probe_at = code.find("function _layerShellSession()").expect("нет _layerShellSession");
    let probe = &code[probe_at..];
    let probe = &probe[..probe.find("\n}\n").expect("конец _layerShellSession")];
    assert!(probe.contains("Meta.is_wayland_compositor()"), "признак без проверки Wayland");
    assert!(probe.contains("'/dbus-1/services/'"), "признак не по файлу активации службы фона");
    assert!(
        code.contains(&format!("const CINNAMON_BG_SERVICE = '{}';", wb::CINNAMON_BG_BUS)),
        "имя службы фона в расширении разошлось с ядром"
    );
    // The same file the core looks for.
    let files = wb::cinnamon_bg_service_files("", "/home/u", "/usr/share");
    assert!(files.iter().all(|f| f.ends_with(&format!("/dbus-1/services/{}.service", wb::CINNAMON_BG_BUS))));
}

// ── order of the wallpaper window and the desktop icon window (X11) ─────────────────────────

fn facts(desktop_type: bool, instance: &str, depth: u8) -> wm::X11ClientFacts {
    wm::X11ClientFacts {
        desktop_type,
        announced_icons: false,
        instance: instance.into(),
        class: instance.into(),
        depth,
    }
}

/// The window role: icons are see-through only with alpha AND with a compositor.
#[test]
fn x11_icons_window_is_see_through_only_with_alpha_and_compositor() {
    use wm::X11StackRole as R;
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 32), true), R::Icons { see_through: true });
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 32), false), R::Icons { see_through: false });
    assert_eq!(wm::x11_stack_role(&facts(true, "desktop_window", 24), true), R::Icons { see_through: false });
    assert_eq!(wm::x11_stack_role(&facts(true, "csd-background", 24), true), R::Background);
    assert_eq!(wm::x11_stack_role(&facts(false, "caja", 32), true), R::Other, "обычное окно Caja");
    // Caja's root property names the desktop window before the type is read.
    let mut announced = facts(false, "", 32);
    announced.announced_icons = true;
    assert_eq!(wm::x11_stack_role(&announced, true), R::Icons { see_through: true });
}

/// Defect of 31.10.2026 (MATE): the wallpaper window is shown later and stands
/// above an opaque Caja — that is right (otherwise the wallpaper is not
/// visible); above a see-through one — goes under it.
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
    // After the restack — in place, a second request is not needed.
    let settled = [(own, R::Own), (caja, R::Icons { see_through: true }), (1, R::Other)];
    assert_eq!(wm::x11_restack_goal(&settled, own), None);
    // Caja recreated the window opaque (the key was turned off) — the wallpaper goes on top.
    let reopened = [(own, R::Own), (caja, R::Icons { see_through: false })];
    assert_eq!(wm::x11_restack_goal(&reopened, own), Some(M::Above(caja)));
    // No icons — nothing to touch.
    assert_eq!(wm::x11_restack_goal(&[(own, R::Own), (1, R::Other)], own), None);
    // Not our window — not our business.
    assert_eq!(wm::x11_restack_goal(&see_through, 0x42), None);
}

/// A window drawing only the background (Cinnamon under XWayland) is always
/// under the wallpaper; several icon windows (xfdesktop has a window per monitor).
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

/// Defect of 31.10.2026 (xfwm4 + nemo-desktop): the manager does not fulfil
/// the stacking request, and the check every 2 s asked again and again. Now —
/// no more than `X11_RESTACK_TRIES` requests with the same stack, then one
/// refusal record and silence; any change of the bottom layer restarts the count.
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
    // Foreign windows outside the bottom layer (a terminal was opened) do not reset the count.
    let other_moved = [(nemo, R::Icons { see_through: true }), (own, R::Own), (1, R::Other)];
    assert_eq!(t.step(&other_moved, own), Some(S::Quiet));
    // nemo-desktop recreated the window — a new attempt.
    let recreated = [(0x1600009, R::Icons { see_through: true }), (own, R::Own)];
    assert_eq!(t.step(&recreated, own), Some(S::Ask(M::Below(0x1600009))));
    // The manager complied — the count is zeroed; it slipped again — we ask again.
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

/// Defect of 31.10.2026 (xfwm4 + nemo-desktop shown before the host): after
/// an acknowledged refusal the icon window is recreated — but only for the
/// «under the icons» goal and once per wallpaper window, without repeating in circles.
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
    // The new icon window is again not under the wallpaper — we ask, acknowledge
    // the refusal, but no longer touch the icon window.
    let stuck = [(0x400076, R::Icons { see_through: true }), (own, R::Own)];
    for _ in 0..=wm::X11_RESTACK_TRIES {
        t.step(&stuck, own);
    }
    assert!(!t.take_icons_recreate(M::Below(0x400076)));
}
