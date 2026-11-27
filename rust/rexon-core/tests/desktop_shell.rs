//! Check of the "compositor + shell" pairing and the owner of the wallpaper layer.
//!
//! ⛔ Neither Noctalia nor DankMaterialShell is installed on the machine, and installing
//! them is forbidden. So the check runs on fake observations: what is fed here is
//! exactly the file, unit, process and variable names these shells
//! leave in a session. The pairing was NOT checked live — that is an honest boundary of
//! the check, not a gap in it: by design the core asks the machine nothing,
//! gathering observations is the caller's job, and it is checked there.
//!
//! Checked: each sign SEPARATELY raises the shell; their combinations
//! do not interfere with each other; unrelated names do not confuse it; a process name by
//! itself does not mean a shell; with a foreign owner the answer "not ours" comes with ready
//! text for the buyer.

mod i18n_pin;

use rexon_core::system::{
    detect_current_de, detect_shell, noctalia_lock_name, noctalia_socket_name, DesktopShell,
    ShellFacts, SHELL_ENV_MARKERS, SHELL_ENV_NAMES,
};
use rexon_core::desktop_shell::{give_label, return_label};
use rexon_core::wallpaper_engine::{
    shell_owner_message, wallpaper_layer_owner, WallpaperOwner,
};

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_string()).collect()
}

/// Observations are empty — no shell.
#[test]
fn пустой_сеанс_шелла_не_даёт() {
    assert_eq!(detect_shell(&ShellFacts::default()), None);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        ..Default::default()
    };
    assert_eq!(detect_shell(&facts), None);
}

/// Bare niri and niri with a shell — ONE compositor value and different pairings.
/// The function exists exactly for this distinction.
#[test]
fn композитор_один_а_связки_разные() {
    assert_eq!(detect_current_de("niri", "", true).as_deref(), Some("niri"));

    let голый = ShellFacts {
        wayland_display: "wayland-1",
        ..Default::default()
    };
    let файлы = s(&["noctalia-wayland-1.sock"]);
    let с_шеллом = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(detect_shell(&голый), None);
    assert_eq!(
        detect_shell(&с_шеллом).map(|d| d.shell),
        Some(DesktopShell::Noctalia)
    );

    // And their background owner is DIFFERENT.
    assert!(wallpaper_layer_owner("niri", "wayland", None, None).is_ours());
    assert!(!wallpaper_layer_owner(
        "niri",
        "wayland",
        Some(DesktopShell::Noctalia),
        None
    )
    .is_ours());
}

// ── Noctalia: each sign separately ─────────────────────────────────────────

#[test]
fn noctalia_по_файлу_блокировки() {
    // The socket is removed only in the destructor and remains after a crash; the flock
    // on .lock is how the shell holds single-instance launching.
    let файлы = s(&["noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("файл блокировки не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.lock в каталоге сеанса"]);
}

#[test]
fn noctalia_по_имени_на_шине() {
    // A name on the bus is never stale — the most reliable sign.
    for имя in ["dev.noctalia.Debug", "dev.noctalia.Mpris"] {
        let имена = s(&["org.freedesktop.Notifications", имя]);
        let facts = ShellFacts {
            dbus_names: &имена,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознано"));
        assert_eq!(d.shell, DesktopShell::Noctalia, "{имя}");
        assert_eq!(d.evidence, vec![format!("имя {имя} на шине")]);
    }
    // Ownerless and conditional names are not counted as a shell: org.freedesktop.Notifications
    // is taken by everybody, and KWinActiveWindow exists only on KDE.
    for чужое in [
        "org.freedesktop.Notifications",
        "org.kde.StatusNotifierWatcher",
        "dev.noctalia.Other",
    ] {
        let имена = s(&[чужое]);
        let facts = ShellFacts {
            dbus_names: &имена,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{чужое}");
    }
}

#[test]
fn noctalia_по_сокету_с_точным_именем() {
    let файлы = s(&["bus", "noctalia-wayland-1.sock", "pipewire-0"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("сокет Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]);
    // The socket name is built by one formula, not rewritten in place.
    assert_eq!(noctalia_socket_name("wayland-1"), "noctalia-wayland-1.sock");
}

#[test]
fn noctalia_по_сокету_без_переменной_дисплея() {
    // We may have been launched without WAYLAND_DISPLAY — we must not go blind because of it,
    // the name shape is recognized anyway. The shell itself substitutes
    // wayland-0 for an empty variable, and the name formula does the same.
    assert_eq!(noctalia_socket_name(""), "noctalia-wayland-0.sock");
    let файлы = s(&["noctalia-wayland-0.sock"]);
    let facts = ShellFacts {
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).map(|d| d.shell),
        Some(DesktopShell::Noctalia)
    );
}

/// Noctalia has no stock unit (the package installs a binary and a .desktop with
/// `Exec=noctalia --daemon`), `noctalia.service` is created only by the
/// NixOS module. The sign is rare, but if the unit exists, it is proof.
#[test]
fn noctalia_по_юниту() {
    let юниты = s(&["pipewire.service", "noctalia.service"]);
    let facts = ShellFacts {
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["запущен юнит noctalia.service"]);
}

// ── DankMaterialShell: each sign separately ────────────────────────────────

#[test]
fn dms_по_каждому_служебному_файлу() {
    for имя in [
        "danklinux-4242.sock",
        "danklinux-4242.pid",
        "danklinux-4242.session",
        // ⚠️ A name without a process number ("danklinux.sock") is deliberately NOT here.
        // That variant could not be verified: the shell takes the socket path from
        // the `DMS_SOCKET` variable, and it is built by the Go backend, whose sources
        // are not in the public DankMaterialShell and danklinux
        // repositories. Recognizing by an unconfirmed pattern is not allowed: a false
        // "shell present" would make the product refuse to set wallpapers and advise
        // turning off foreign ones — on a machine where this shell does not exist at all.
        // If it shows up live — extend `detect_shell` and add it here.
    ] {
        let файлы = s(&["bus", имя]);
        let facts = ShellFacts {
            runtime_files: &файлы,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознан"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{имя}");
        assert_eq!(d.evidence, vec![format!("файл {имя} в каталоге сеанса")]);
    }
}

#[test]
fn dms_по_юниту() {
    let юниты = s(&["dms.service"]);
    let facts = ShellFacts {
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит dms не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["запущен юнит dms.service"]);
}

#[test]
fn dms_по_метке_окружения() {
    // We were launched FROM the shell — we inherited the marker.
    for имя in ["DMS_SOCKET", "DMS_EXECUTABLE"] {
        let окружение = [(имя, "/run/user/1000/danklinux-77.sock")];
        let facts = ShellFacts {
            env: &окружение,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознана"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{имя}");
        assert_eq!(d.evidence, vec![format!("переменная {имя} в окружении")]);
    }
    // `QS_APP_ID` is an environment VARIABLE of the `qs` process (set by the
    // `dankgo` launch module), and it is a sign only with the EXACT DMS value.
    let окружение = [("QS_APP_ID", "com.danklinux.dms")];
    let facts = ShellFacts {
        env: &окружение,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("QS_APP_ID=com.danklinux.dms не опознана");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["переменная QS_APP_ID=com.danklinux.dms в окружении"]);

    // An empty value is not a sign; foreign DMS_* are not either (they are input
    // settings of the shell itself, set by the user); a `QS_APP_ID`
    // of another program on the same launch module (an example from a comment
    // of `dankgo` itself) is not DMS either.
    for (имя, значение) in [
        ("DMS_SOCKET", ""),
        ("DMS_LOG_LEVEL", "debug"),
        ("DMS_SHELL_DIR", "/opt/dms"),
        ("QS_APP_ID", "com.danklinux.dankcalendar"),
        ("QS_APP_ID", ""),
    ] {
        let окружение = [(имя, значение)];
        let facts = ShellFacts {
            env: &окружение,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{имя}={значение}");
    }
}

/// The list of variables for live gathering has not drifted apart from the sign table.
#[test]
fn имена_переменных_для_сбора_совпадают_с_таблицей() {
    let mut из_таблицы: Vec<&str> = SHELL_ENV_MARKERS
        .iter()
        .flat_map(|(_, m)| m.iter().map(|e| e.name))
        .collect();
    let mut собираемые: Vec<&str> = SHELL_ENV_NAMES.to_vec();
    из_таблицы.sort();
    собираемые.sort();
    assert_eq!(из_таблицы, собираемые);
}

/// The second socket of the Noctalia daemon (`noctalia-dmenu-<display>.sock`) is also
/// the shell (created by the daemon itself), but in the confirmation the EXACT name of
/// the control socket comes first, if it exists.
#[test]
fn noctalia_точное_имя_раньше_dmenu() {
    let файлы = s(&["noctalia-dmenu-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("dmenu-сокет не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-dmenu-wayland-1.sock в каталоге сеанса"]);

    // The order in the directory is alphabetical, dmenu comes earlier; the confirmation
    // still names the exact socket of our display.
    let файлы = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).unwrap().evidence,
        vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]
    );
    // The exact lock name also comes before the fallback.
    let файлы = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).unwrap().evidence,
        vec!["файл noctalia-wayland-1.lock в каталоге сеанса"]
    );
    assert_eq!(noctalia_lock_name("wayland-1"), "noctalia-wayland-1.lock");
    assert_eq!(noctalia_lock_name(""), "noctalia-wayland-0.lock");
}

// ── process name: a confirmation, but not proof ────────────────────────────

#[test]
fn одно_имя_процесса_шелла_не_означает() {
    // `noctalia msg …` and `dms ipc call …` are the shell's command line, not
    // the shell itself: a process with the same name lives for milliseconds. Taking that
    // for a live shell is not allowed, otherwise our own state check would declare
    // a foreign one the background owner.
    // We do not take `qs` and `quickshell` at all: ANY Quickshell-based shell runs under them,
    // and it can be started by hand, without DankMaterialShell.
    for имя in ["noctalia", "dms", "qs", "quickshell"] {
        let процессы = s(&[имя, "kwin_wayland"]);
        let facts = ShellFacts {
            process_names: &процессы,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "имя процесса {имя}");
    }
}

#[test]
fn имя_процесса_попадает_в_подтверждения_к_доказательству() {
    let файлы = s(&["noctalia-wayland-1.sock"]);
    let процессы = s(&["noctalia"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        process_names: &процессы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(
        d.evidence,
        vec![
            "файл noctalia-wayland-1.sock в каталоге сеанса".to_string(),
            "процесс noctalia".to_string(),
        ]
    );
}

// ── combinations and unrelated names ───────────────────────────────────────

#[test]
fn все_признаки_разом_складываются_а_не_путаются() {
    let файлы = s(&["danklinux-77.sock", "danklinux-77.pid"]);
    let юниты = s(&["dms.service"]);
    let процессы = s(&["dms", "qs"]);
    let окружение = [("DMS_SOCKET", "/run/user/1000/danklinux-77.sock")];
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        dbus_names: &[],
        process_names: &процессы,
        env: &окружение,
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(
        d.evidence,
        vec![
            "файл danklinux-77.sock в каталоге сеанса".to_string(),
            "запущен юнит dms.service".to_string(),
            "переменная DMS_SOCKET в окружении".to_string(),
            "процесс dms".to_string(),
        ]
    );
}

#[test]
fn похожие_имена_не_считаются() {
    // None of these names is a shell.
    let файлы = s(&[
        "noctalia.sock",        // without a display name — not our shape
        "noctalia-wayland-1",   // without an extension
        "danklinux77.sock",     // without a separator
        "danklinux-77.log",     // a foreign extension
        "dbus-1",
    ]);
    let юниты = s(&["dms.socket", "noctalia-backup.service", "dms-shell.timer"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        ..Default::default()
    };
    assert_eq!(detect_shell(&facts), None);
}

#[test]
fn признаки_обоих_шеллов_разом_решаются_числом_доказательств() {
    // Leftovers of a past session: one file from Noctalia and the full DMS set.
    let файлы = s(&["noctalia-wayland-1.sock", "danklinux-9.sock"]);
    let юниты = s(&["dms.service"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    // But the main answer does not depend on the choice: the background is foreign in both cases.
    for шелл in DesktopShell::all() {
        assert!(!wallpaper_layer_owner("niri", "wayland", Some(*шелл), None).is_ours());
    }
}

// ── owner of the wallpaper layer ───────────────────────────────────────────

#[test]
fn без_шелла_фон_наш() {
    assert_eq!(
        wallpaper_layer_owner("niri", "wayland", None, None),
        WallpaperOwner::Rexon
    );
    assert_eq!(
        wallpaper_layer_owner("kde", "wayland", None, Some(true)),
        WallpaperOwner::Rexon
    );
}

#[test]
fn с_шеллом_фон_чужой() {
    let o = wallpaper_layer_owner("niri", "wayland", Some(DesktopShell::Noctalia), None);
    assert_eq!(o, WallpaperOwner::Shell(DesktopShell::Noctalia));
    assert!(!o.is_ours());
    assert_eq!(o.shell(), Some(DesktopShell::Noctalia));
    // Noctalia lives on Plasma too — the compositor has nothing to do with it.
    assert!(!wallpaper_layer_owner("kde", "wayland", Some(DesktopShell::Noctalia), None)
        .is_ours());
}

#[test]
fn выключенные_обои_шелла_возвращают_фон_нам() {
    // The buyer did what we asked — the background is ours again.
    assert_eq!(
        wallpaper_layer_owner(
            "niri",
            "wayland",
            Some(DesktopShell::DankMaterialShell),
            Some(false)
        ),
        WallpaperOwner::Rexon
    );
    // "Did not find out" is NOT "turned off": the cautious side.
    assert_eq!(
        wallpaper_layer_owner("niri", "wayland", Some(DesktopShell::DankMaterialShell), None),
        WallpaperOwner::Shell(DesktopShell::DankMaterialShell)
    );
}

#[test]
fn на_x11_и_под_mutter_фон_всегда_наш() {
    // Both shells are Wayland only; on X11 wallpapers are placed as a window.
    assert_eq!(
        wallpaper_layer_owner("niri", "x11", Some(DesktopShell::Noctalia), None),
        WallpaperOwner::Rexon
    );
    // Mutter and Muffin cannot do wlr-layer-shell: there is nothing to take the wallpaper layer
    // with there, the only way is a shell extension.
    for de in ["gnome", "cinnamon"] {
        assert_eq!(
            wallpaper_layer_owner(de, "wayland", Some(DesktopShell::Noctalia), None),
            WallpaperOwner::Rexon,
            "{de}"
        );
    }
}

// ── text for the buyer ─────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn при_чужом_владельце_есть_что_сказать() {
    // The text is in Russian from the real catalog.
    let _lang = i18n_pin::ru();
    for шелл in DesktopShell::all() {
        let t = shell_owner_message(*шелл);
        assert!(t.contains("Фон сейчас рисует"), "{t}");
        assert!(t.contains(шелл.label()), "{t}");
        // What to press — that very button, with the same word as on the button.
        assert!(t.contains("«Отдать обои Rexon»"), "{t}");
        assert!(t.contains("«Wallpaper Engine»"), "{t}");
        assert!(t.contains("примените обои Rexon ещё раз"), "{t}");
        // Jargon (paths, keys, identifiers) has no place in the text.
        for жаргон in ["~/", "settings.toml", "screenPreferences", "enabled", "noctalia:"] {
            assert!(!t.contains(жаргон), "{жаргон} в «{t}»");
        }
    }
    assert_eq!(give_label(), "Отдать обои Rexon");
    assert_eq!(return_label(), "Вернуть оболочке");
}

/// Shell identifiers do not drift apart from the names.
#[test]
fn идентификаторы_и_имена() {
    assert_eq!(DesktopShell::Noctalia.as_str(), "noctalia");
    assert_eq!(DesktopShell::DankMaterialShell.as_str(), "dms");
    assert_eq!(DesktopShell::Noctalia.label(), "Noctalia");
    assert_eq!(DesktopShell::DankMaterialShell.label(), "DankMaterialShell");
    assert_eq!(DesktopShell::all().len(), 2);
    assert_eq!(
        serde_json::to_string(&DesktopShell::DankMaterialShell).unwrap(),
        "\"dms\""
    );
}

/// The same advice in English: without Russian letters, with the same button.
#[test]
fn english_shell_owner_message_has_no_russian() {
    let _lang = i18n_pin::en();
    for шелл in DesktopShell::all() {
        let t = shell_owner_message(*шелл);
        assert!(
            !t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
            "{t}"
        );
        assert!(t.starts_with("The background is currently drawn by "), "{t}");
        assert!(t.contains("press “Give Wallpaper to Rexon”"), "{t}");
        assert!(t.ends_with("apply the Rexon wallpaper again."), "{t}");
        assert!(t.contains(шелл.label()), "{t}");
    }
    assert_eq!(give_label(), "Give Wallpaper to Rexon");
    assert_eq!(return_label(), "Return to Shell");
}

// ════════════════════════════════════════════════════════════════════════════
// Step 2: «Отдать обои Rexon» / «Вернуть оболочке»
// ════════════════════════════════════════════════════════════════════════════
//
// ⛔ There are no live shells on the machine. The edit is checked on fake files of
// the forms in which the shells themselves write them (Noctalia — toml++, DMS —
// `JSON.stringify` with indentation), and on a private HOME in a temporary directory:
// "before → after → back" must return the file BIT FOR BIT.

use rexon_core::desktop_shell::{
    backup_path, dms_disable, dms_draws_wallpaper, dms_restore, give_back, hand_over,
    live_runtime_files, noctalia_disable, noctalia_draws_wallpaper, noctalia_restore,
    read_handover, shell_banner, shell_draws_wallpaper, shell_wallpaper_state, EditError,
    Previous, Restored, ShellHome,
};

/// A round trip on one text: turned off, then exactly the original.
fn noctalia_round_trip(before: Option<&str>) -> (String, Previous) {
    let (after, prev) = noctalia_disable(before).expect("правка отвергнута");
    assert_eq!(
        noctalia_draws_wallpaper(&[], Some(&after)),
        Some(false),
        "после правки обои Noctalia всё ещё включены:\n{after}"
    );
    match (noctalia_restore(Some(&after), &prev).expect("возврат отвергнут"), before) {
        (Restored::Write(back), Some(b)) => assert_eq!(back, b, "возврат не бит в бит"),
        (Restored::Delete, None) => {}
        (r, b) => panic!("возврат {r:?} при исходнике {b:?}"),
    }
    (after, prev)
}

#[test]
fn noctalia_правка_во_всех_формах_файла_и_возврат_бит_в_бит() {
    // No state file — we create it, the restore deletes it.
    let (after, prev) = noctalia_round_trip(None);
    assert_eq!(after, "[wallpaper]\nenabled = false\n");
    assert_eq!(prev, Previous::FileAbsent);

    // This is how toml++ writes: tables separated by an empty line, wallpaper subtables without
    // `[wallpaper]` itself (it has no keys of its own).
    let toml_pp = "config_version = 3\n\n[theme]\nmode = \"dark\"\n\n\
                   [wallpaper.default]\npath = \"/home/u/a.png\"\n\n\
                   [wallpaper.last]\npath = \"/home/u/a.png\"\n";
    let (after, prev) = noctalia_round_trip(Some(toml_pp));
    assert!(after.starts_with(toml_pp), "остальной файл тронут:\n{after}");
    assert_eq!(&after[toml_pp.len()..], "\n[wallpaper]\nenabled = false\n");
    assert_eq!(prev, Previous::KeyAbsent { parent_created: true, prefix: "\n".into() });

    // The table exists, the key does not — the line goes right under the header.
    let hdr = "[wallpaper]\nfill_mode = \"crop\"\n\n[bar.default]\nposition = \"top\"\n";
    let (after, prev) = noctalia_round_trip(Some(hdr));
    assert_eq!(
        after,
        "[wallpaper]\nenabled = false\nfill_mode = \"crop\"\n\n[bar.default]\nposition = \"top\"\n"
    );
    assert_eq!(prev, Previous::KeyAbsent { parent_created: false, prefix: String::new() });

    // The key exists — ONLY the value changes, the comment and indentation stay.
    let key = "[wallpaper]\n  enabled   =   true   # из настроек\nfill_mode = \"crop\"\n";
    let (after, prev) = noctalia_round_trip(Some(key));
    assert_eq!(
        after,
        "[wallpaper]\n  enabled   =   false   # из настроек\nfill_mode = \"crop\"\n"
    );
    assert_eq!(prev, Previous::Value { raw: "true".into() });

    // Without a trailing line break — and the header without one too.
    noctalia_round_trip(Some("[theme]\nmode = \"dark\""));
    noctalia_round_trip(Some("[wallpaper]"));
    // An empty file.
    noctalia_round_trip(Some(""));
    // "[wallpaper]" inside a multi-line string is not a header.
    noctalia_round_trip(Some("note = \"\"\"\n[wallpaper]\nenabled = true\n\"\"\"\n"));
    // A per-monitor table is not ours.
    noctalia_round_trip(Some("[wallpaper.monitor.DP-1]\nenabled = true\n"));
}

#[test]
fn noctalia_небезопасные_формы_отвергаются_без_порчи() {
    for (текст, ошибка) in [
        ("wallpaper.enabled = true\n", EditError::UnsafeForm),
        ("wallpaper = { enabled = true }\n", EditError::UnsafeForm),
        ("[wallpaper]\nenabled = \"yes\"\n", EditError::UnsafeForm),
        ("wallpaper = 5\n", EditError::UnsafeForm),
        ("[wallpaper\nenabled = true\n", EditError::Unparsable),
    ] {
        assert_eq!(noctalia_disable(Some(текст)).err(), Some(ошибка), "{текст}");
    }
}

#[test]
fn noctalia_возврат_после_того_как_шелл_переписал_файл() {
    // We set false in an empty table; the shell later rewrote the file in its own
    // format and added keys. The restore removes only our key.
    let prev = Previous::KeyAbsent { parent_created: true, prefix: "\n".into() };
    let rewritten = "[theme]\nmode = \"dark\"\n\n[wallpaper]\nenabled = false\n\n[wallpaper.last]\npath = \"/x.png\"\n";
    let Restored::Write(back) = noctalia_restore(Some(rewritten), &prev).unwrap() else {
        panic!("файл не должен удаляться");
    };
    assert_eq!(back, "[theme]\nmode = \"dark\"\n\n\n[wallpaper.last]\npath = \"/x.png\"\n");
    assert_eq!(noctalia_draws_wallpaper(&[], Some(&back)), Some(true));

    // There was a value, but the shell removed the key — we put the value back.
    let prev = Previous::Value { raw: "true".into() };
    let Restored::Write(back) = noctalia_restore(Some("[theme]\nmode = \"dark\"\n"), &prev).unwrap() else {
        panic!()
    };
    assert_eq!(noctalia_draws_wallpaper(&[], Some(&back)), Some(true));
    assert!(back.starts_with("[theme]\nmode = \"dark\"\n"), "{back}");

    // The file was deleted without us: "there was no file" stays so.
    assert_eq!(noctalia_restore(None, &Previous::FileAbsent).unwrap(), Restored::Delete);
}

#[test]
fn noctalia_слои_настроек_и_кто_побеждает() {
    let on = "[wallpaper]\nenabled = true\n".to_string();
    let off = "[wallpaper]\nenabled = false\n".to_string();
    assert_eq!(noctalia_draws_wallpaper(&[], None), Some(true), "по умолчанию включены");
    assert_eq!(noctalia_draws_wallpaper(&[off.clone()], None), Some(false));
    // The last configuration layer beats the previous one.
    assert_eq!(noctalia_draws_wallpaper(&[off.clone(), on.clone()], None), Some(true));
    // The state file overrides ~/.config — that is exactly why we write to it.
    assert_eq!(noctalia_draws_wallpaper(&[off.clone()], Some(&on)), Some(true));
    assert_eq!(noctalia_draws_wallpaper(&[on.clone()], Some(&off)), Some(false));
    // A per-monitor switch does not change the overall answer: on the other
    // outputs the shell draws.
    let per = "[wallpaper.monitor.DP-1]\nenabled = false\n";
    assert_eq!(noctalia_draws_wallpaper(&[], Some(per)), Some(true));
    // A broken layer is "I don't know", not a guess.
    assert_eq!(noctalia_draws_wallpaper(&["[[[".to_string()], None), None);
}

/// This is how DMS writes: `JSON.stringify(…, null, 2)`.
const DMS_PRETTY: &str = "{\n  \"currentThemeName\": \"purple\",\n  \"screenPreferences\": {\n    \"dankBar\": [\n      \"all\"\n    ],\n    \"wallpaper\": [\n      \"all\"\n    ]\n  },\n  \"showOnLastDisplay\": {}\n}\n";

fn dms_round_trip(before: Option<&str>) -> (String, Previous) {
    let (after, prev) = dms_disable(before).expect("правка отвергнута");
    assert_eq!(dms_draws_wallpaper(Some(&after)), Some(false), "после правки:\n{after}");
    match (dms_restore(Some(&after), &prev).expect("возврат отвергнут"), before) {
        (Restored::Write(back), Some(b)) => assert_eq!(back, b, "возврат не бит в бит"),
        (Restored::Delete, None) => {}
        (r, b) => panic!("возврат {r:?} при исходнике {b:?}"),
    }
    (after, prev)
}

#[test]
fn dms_правка_во_всех_формах_файла_и_возврат_бит_в_бит() {
    // The key exists — only its value changes, a multi-line one as a whole.
    let (after, prev) = dms_round_trip(Some(DMS_PRETTY));
    assert_eq!(after, DMS_PRETTY.replace("\"wallpaper\": [\n      \"all\"\n    ]", "\"wallpaper\": []"));
    assert_eq!(prev, Previous::Value { raw: "[\n      \"all\"\n    ]".into() });

    // Wallpaper on only one screen — it still draws, we still turn it off and
    // restore exactly that screen.
    let one = "{\"screenPreferences\":{\"wallpaper\":[\"DP-1\"]}}";
    let (_, prev) = dms_round_trip(Some(one));
    assert_eq!(prev, Previous::Value { raw: "[\"DP-1\"]".into() });

    // The object exists, the key does not.
    let (after, prev) = dms_round_trip(Some("{\n  \"screenPreferences\": {\n    \"dankBar\": []\n  }\n}\n"));
    assert_eq!(after, "{\n  \"screenPreferences\": {\n    \"wallpaper\": [],\n    \"dankBar\": []\n  }\n}\n");
    assert_eq!(prev, Previous::KeyAbsent { parent_created: false, prefix: String::new() });

    // No object at all.
    let (_, prev) = dms_round_trip(Some("{\n  \"currentThemeName\": \"purple\"\n}\n"));
    assert_eq!(prev, Previous::KeyAbsent { parent_created: true, prefix: String::new() });
    // Empty objects.
    dms_round_trip(Some("{}"));
    dms_round_trip(Some("{\"screenPreferences\": {}}\n"));
    // Strings with curly braces and escaped quotes do not confuse
    // the boundary parsing.
    dms_round_trip(Some("{\"a\": \"}{\\\"]\", \"screenPreferences\": {\"wallpaper\": [\"x}\"]}}"));
    // No file — we create it, the restore deletes it.
    let (_, prev) = dms_round_trip(None);
    assert_eq!(prev, Previous::FileAbsent);
}

#[test]
fn dms_небезопасные_формы_отвергаются_без_порчи() {
    for (текст, ошибка) in [
        ("{\"screenPreferences\": null}", EditError::UnsafeForm),
        ("{\"screenPreferences\": []}", EditError::UnsafeForm),
        ("[1, 2]", EditError::UnsafeForm),
        ("{\"screenPreferences\": {", EditError::Unparsable),
    ] {
        assert_eq!(dms_disable(Some(текст)).err(), Some(ошибка), "{текст}");
    }
}

#[test]
fn dms_кто_рисует() {
    assert_eq!(dms_draws_wallpaper(None), Some(true), "без файла — умолчания, обои есть");
    assert_eq!(dms_draws_wallpaper(Some("{}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some(DMS_PRETTY)), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":null}}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":[\"DP-1\"]}}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":[]}}")), Some(false));
    assert_eq!(dms_draws_wallpaper(Some("{")), None);
}

// ── on disk, with a private HOME ──────────────────────────────────────────

/// Our own temporary HOME per check (in the temporary directory — RAM; the files are
/// tiny). The live profile is not touched: all roots point here.
struct PrivateHome {
    root: std::path::PathBuf,
    home: ShellHome,
}

impl PrivateHome {
    fn new(tag: &str) -> PrivateHome {
        let root = std::env::temp_dir().join(format!("rexon-shell-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home = ShellHome { home: root.to_string_lossy().into_owned(), ..Default::default() };
        PrivateHome { root, home }
    }
}

impl Drop for PrivateHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn put(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn на_диске_noctalia_до_после_обратно_бит_в_бит() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("noct");
    let state = h.home.noctalia_state_file();
    assert!(state.ends_with(".local/state/noctalia/settings.toml"), "{}", state.display());
    let before = "config_version = 3\n\n[wallpaper.default]\npath = \"/a.png\"\n";
    put(&state, before);
    // The configuration in ~/.config enables wallpapers explicitly — the state will override it.
    put(&h.home.noctalia_config_dir().join("config.toml"), "[wallpaper]\nenabled = true\n");
    std::fs::set_permissions(&state, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();

    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(true));
    let msg = hand_over(DesktopShell::Noctalia, &h.home).expect("не отдали");
    assert!(msg.contains("Noctalia"), "{msg}");
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(false));
    // The backup copy is the former contents; the file permissions are preserved.
    assert_eq!(std::fs::read_to_string(backup_path(&state)).unwrap(), before);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&state).unwrap().permissions().mode() & 0o777, 0o600);
    // ~/.config is untouched.
    assert_eq!(
        std::fs::read_to_string(h.home.noctalia_config_dir().join("config.toml")).unwrap(),
        "[wallpaper]\nenabled = true\n"
    );
    let rec = read_handover(&h.home, DesktopShell::Noctalia).expect("памятки нет");
    assert_eq!(rec.shell, "noctalia");
    // A repeated «Отдать» does not spoil the note: the former value stays what
    // was there BEFORE Rexon.
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    assert_eq!(read_handover(&h.home, DesktopShell::Noctalia), Some(rec));

    let msg = give_back(DesktopShell::Noctalia, &h.home).expect("не вернули");
    assert!(msg.contains("back on"), "{msg}");
    assert_eq!(std::fs::read_to_string(&state).unwrap(), before, "возврат не бит в бит");
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_none(), "памятка осталась после возврата");
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(true));
    // A second restore — an honest "nothing to restore".
    assert!(give_back(DesktopShell::Noctalia, &h.home).is_err());
}

#[test]
fn на_диске_noctalia_без_файла_состояния_и_xdg_корни() {
    let _lang = i18n_pin::en();
    let mut h = PrivateHome::new("noct-xdg");
    h.home.xdg_state_home = h.root.join("st").to_string_lossy().into_owned();
    h.home.noctalia_state_home = h.root.join("nst").to_string_lossy().into_owned();
    // NOCTALIA_STATE_HOME beats XDG_STATE_HOME — as in the shell itself.
    assert_eq!(h.home.noctalia_state_file(), h.root.join("nst/noctalia/settings.toml"));
    // While the Rexon note is in XDG_STATE_HOME.
    assert_eq!(h.home.handover_file(), h.root.join("st/rexon/shell-wallpaper.json"));
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    assert!(h.home.noctalia_state_file().is_file());
    assert!(!backup_path(&h.home.noctalia_state_file()).exists(), "копировать было нечего");
    give_back(DesktopShell::Noctalia, &h.home).unwrap();
    assert!(!h.home.noctalia_state_file().exists(), "заведённый Rexon файл не убран");
}

#[test]
fn на_диске_dms_до_после_обратно_бит_в_бит_и_ссылка_цела() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("dms");
    let file = h.home.dms_settings_file();
    assert!(file.ends_with(".config/DankMaterialShell/settings.json"));
    // The settings live in a "store" and are linked in with a symlink.
    let real = h.root.join("dotfiles/dms.json");
    put(&real, DMS_PRETTY);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &file).unwrap();

    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert!(std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink(), "ссылку заменили файлом");
    assert_eq!(dms_draws_wallpaper(Some(&std::fs::read_to_string(&real).unwrap())), Some(false));
    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(&real).unwrap(), DMS_PRETTY, "возврат не бит в бит");
    assert!(std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
}

#[test]
fn на_диске_только_для_чтения_отказ_и_ничего_не_тронуто() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("ro");
    let file = h.home.dms_settings_file();
    put(&file, DMS_PRETTY);
    std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o444)).unwrap();
    let err = hand_over(DesktopShell::DankMaterialShell, &h.home).expect_err("правка закрытого файла");
    assert!(err.contains("read-only"), "{err}");
    assert!(err.contains("Nothing was changed"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), DMS_PRETTY);
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_none(), "памятка осталась при отказе");
    assert!(!backup_path(&file).exists());
}

#[test]
fn на_диске_неразборный_файл_отказ_и_ничего_не_тронуто() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("broken");
    let file = h.home.noctalia_state_file();
    put(&file, "[wallpaper\n");
    let err = hand_over(DesktopShell::Noctalia, &h.home).expect_err("правка сломанного файла");
    assert!(err.contains("Noctalia"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "[wallpaper\n");
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_none());
}

// ── live gathering and the banner on the tab ──────────────────────────────

#[test]
fn протухшие_служебные_файлы_отсеиваются() {
    let files = s(&[
        "bus",
        "danklinux-100.sock",
        "danklinux-100.pid",
        "danklinux-200.sock",
        "noctalia-wayland-1.sock",
        "noctalia-wayland-1.lock",
    ]);
    let alive = |pid: u32| pid == 100;
    assert_eq!(
        live_runtime_files(&files, alive, false),
        s(&["bus", "danklinux-100.sock", "danklinux-100.pid"])
    );
    assert_eq!(
        live_runtime_files(&files, |_| false, true),
        s(&["bus", "noctalia-wayland-1.sock", "noctalia-wayland-1.lock"])
    );
}

#[test]
fn полоса_только_при_живом_шелле() {
    let _lang = i18n_pin::en();
    let n = DesktopShell::Noctalia;
    // No shell — no banner, even if a note remained from a past session.
    assert!(!shell_banner(None, WallpaperOwner::Rexon, Some(n)).visible);
    // The shell holds the background — «Отдать».
    let b = shell_banner(Some(n), WallpaperOwner::Shell(n), None);
    assert!(b.visible && b.can_give && !b.can_return, "{b:?}");
    assert!(b.text.contains("Noctalia") && b.text.contains("Give Wallpaper to Rexon"), "{}", b.text);
    assert_eq!(b.shell, "noctalia");
    // The background is ours because Rexon turned off this shell's wallpapers — «Вернуть».
    let b = shell_banner(Some(n), WallpaperOwner::Rexon, Some(n));
    assert!(b.visible && !b.can_give && b.can_return, "{b:?}");
    assert!(b.text.contains("Return to Shell"), "{}", b.text);
    // The person turned off the shell's wallpapers themselves — nothing to argue about.
    assert!(!shell_banner(Some(n), WallpaperOwner::Rexon, None).visible);
    // A note about ANOTHER shell does not apply to this one.
    assert!(!shell_banner(Some(n), WallpaperOwner::Rexon, Some(DesktopShell::DankMaterialShell)).visible);
}

#[test]
fn состояние_сеанса_по_файлам_до_и_после_передачи() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("state");
    let det = |shell| Some(rexon_core::system::ShellDetection { shell, evidence: vec![] });
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert_eq!(st.owner, WallpaperOwner::Shell(DesktopShell::DankMaterialShell));
    assert!(st.banner.can_give);

    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(st.owner.is_ours(), "после передачи фон всё ещё чужой");
    assert_eq!(st.handed_over, Some(DesktopShell::DankMaterialShell));
    assert!(st.banner.can_return && !st.banner.can_give);

    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(!st.owner.is_ours());
    // On X11 the same shell does not hold the background — and there is no banner.
    let st = shell_wallpaper_state("niri", "x11", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(st.owner.is_ours() && !st.banner.visible);
}

/// The notes of two shells live side by side: turning off one's wallpapers does not erase
/// the memory of the other, and restoring one does not touch the second.
#[test]
fn на_диске_два_шелла_памятки_не_мешают_друг_другу() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("two");
    let noct_before = "[theme]\nmode = \"dark\"\n";
    put(&h.home.noctalia_state_file(), noct_before);
    put(&h.home.dms_settings_file(), DMS_PRETTY);
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_some());
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_some());

    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(h.home.dms_settings_file()).unwrap(), DMS_PRETTY);
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_none());
    // Noctalia is still turned off by us, and its note is intact.
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(false));
    give_back(DesktopShell::Noctalia, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(h.home.noctalia_state_file()).unwrap(), noct_before);
    assert!(!h.home.handover_file().exists(), "пустой файл памяток не убран");
}
