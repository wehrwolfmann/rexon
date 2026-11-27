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
fn empty_session_gives_no_shell() {
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
fn same_compositor_different_bundles() {
    assert_eq!(detect_current_de("niri", "", true).as_deref(), Some("niri"));

    let bare = ShellFacts {
        wayland_display: "wayland-1",
        ..Default::default()
    };
    let file_list = s(&["noctalia-wayland-1.sock"]);
    let with_shell = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        ..Default::default()
    };
    assert_eq!(detect_shell(&bare), None);
    assert_eq!(
        detect_shell(&with_shell).map(|d| d.shell),
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
fn noctalia_by_lock_file() {
    // The socket is removed only in the destructor and remains after a crash; the flock
    // on .lock is how the shell holds single-instance launching.
    let file_list = s(&["noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("файл блокировки не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.lock в каталоге сеанса"]);
}

#[test]
fn noctalia_by_bus_name() {
    // A name on the bus is never stale — the most reliable sign.
    for label_name in ["dev.noctalia.Debug", "dev.noctalia.Mpris"] {
        let names = s(&["org.freedesktop.Notifications", label_name]);
        let facts = ShellFacts {
            dbus_names: &names,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{label_name} не опознано"));
        assert_eq!(d.shell, DesktopShell::Noctalia, "{label_name}");
        assert_eq!(d.evidence, vec![format!("имя {label_name} на шине")]);
    }
    // Ownerless and conditional names are not counted as a shell: org.freedesktop.Notifications
    // is taken by everybody, and KWinActiveWindow exists only on KDE.
    for foreign_value in [
        "org.freedesktop.Notifications",
        "org.kde.StatusNotifierWatcher",
        "dev.noctalia.Other",
    ] {
        let names = s(&[foreign_value]);
        let facts = ShellFacts {
            dbus_names: &names,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{foreign_value}");
    }
}

#[test]
fn noctalia_by_socket_with_exact_name() {
    let file_list = s(&["bus", "noctalia-wayland-1.sock", "pipewire-0"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("сокет Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]);
    // The socket name is built by one formula, not rewritten in place.
    assert_eq!(noctalia_socket_name("wayland-1"), "noctalia-wayland-1.sock");
}

#[test]
fn noctalia_by_socket_without_display_variable() {
    // We may have been launched without WAYLAND_DISPLAY — we must not go blind because of it,
    // the name shape is recognized anyway. The shell itself substitutes
    // wayland-0 for an empty variable, and the name formula does the same.
    assert_eq!(noctalia_socket_name(""), "noctalia-wayland-0.sock");
    let file_list = s(&["noctalia-wayland-0.sock"]);
    let facts = ShellFacts {
        runtime_files: &file_list,
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
fn noctalia_by_unit() {
    let units = s(&["pipewire.service", "noctalia.service"]);
    let facts = ShellFacts {
        active_user_units: &units,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["запущен юнит noctalia.service"]);
}

// ── DankMaterialShell: each sign separately ────────────────────────────────

#[test]
fn dms_by_each_service_file() {
    for label_name in [
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
        let file_list = s(&["bus", label_name]);
        let facts = ShellFacts {
            runtime_files: &file_list,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{label_name} не опознан"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{label_name}");
        assert_eq!(d.evidence, vec![format!("файл {label_name} в каталоге сеанса")]);
    }
}

#[test]
fn dms_by_unit() {
    let units = s(&["dms.service"]);
    let facts = ShellFacts {
        active_user_units: &units,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит dms не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["запущен юнит dms.service"]);
}

#[test]
fn dms_by_environment_marker() {
    // We were launched FROM the shell — we inherited the marker.
    for label_name in ["DMS_SOCKET", "DMS_EXECUTABLE"] {
        let environment = [(label_name, "/run/user/1000/danklinux-77.sock")];
        let facts = ShellFacts {
            env: &environment,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{label_name} не опознана"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{label_name}");
        assert_eq!(d.evidence, vec![format!("переменная {label_name} в окружении")]);
    }
    // `QS_APP_ID` is an environment VARIABLE of the `qs` process (set by the
    // `dankgo` launch module), and it is a sign only with the EXACT DMS value.
    let environment = [("QS_APP_ID", "com.danklinux.dms")];
    let facts = ShellFacts {
        env: &environment,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("QS_APP_ID=com.danklinux.dms не опознана");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["переменная QS_APP_ID=com.danklinux.dms в окружении"]);

    // An empty value is not a sign; foreign DMS_* are not either (they are input
    // settings of the shell itself, set by the user); a `QS_APP_ID`
    // of another program on the same launch module (an example from a comment
    // of `dankgo` itself) is not DMS either.
    for (label_name, field_value) in [
        ("DMS_SOCKET", ""),
        ("DMS_LOG_LEVEL", "debug"),
        ("DMS_SHELL_DIR", "/opt/dms"),
        ("QS_APP_ID", "com.danklinux.dankcalendar"),
        ("QS_APP_ID", ""),
    ] {
        let environment = [(label_name, field_value)];
        let facts = ShellFacts {
            env: &environment,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{label_name}={field_value}");
    }
}

/// The list of variables for live gathering has not drifted apart from the sign table.
#[test]
fn collected_variable_names_match_table() {
    let mut from_table: Vec<&str> = SHELL_ENV_MARKERS
        .iter()
        .flat_map(|(_, m)| m.iter().map(|e| e.name))
        .collect();
    let mut collected: Vec<&str> = SHELL_ENV_NAMES.to_vec();
    from_table.sort();
    collected.sort();
    assert_eq!(from_table, collected);
}

/// The second socket of the Noctalia daemon (`noctalia-dmenu-<display>.sock`) is also
/// the shell (created by the daemon itself), but in the confirmation the EXACT name of
/// the control socket comes first, if it exists.
#[test]
fn noctalia_exact_name_before_dmenu() {
    let file_list = s(&["noctalia-dmenu-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("dmenu-сокет не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-dmenu-wayland-1.sock в каталоге сеанса"]);

    // The order in the directory is alphabetical, dmenu comes earlier; the confirmation
    // still names the exact socket of our display.
    let file_list = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).unwrap().evidence,
        vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]
    );
    // The exact lock name also comes before the fallback.
    let file_list = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
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
fn process_name_alone_does_not_mean_shell() {
    // `noctalia msg …` and `dms ipc call …` are the shell's command line, not
    // the shell itself: a process with the same name lives for milliseconds. Taking that
    // for a live shell is not allowed, otherwise our own state check would declare
    // a foreign one the background owner.
    // We do not take `qs` and `quickshell` at all: ANY Quickshell-based shell runs under them,
    // and it can be started by hand, without DankMaterialShell.
    for label_name in ["noctalia", "dms", "qs", "quickshell"] {
        let processes = s(&[label_name, "kwin_wayland"]);
        let facts = ShellFacts {
            process_names: &processes,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "имя процесса {label_name}");
    }
}

#[test]
fn process_name_counts_as_confirmation_of_evidence() {
    let file_list = s(&["noctalia-wayland-1.sock"]);
    let processes = s(&["noctalia"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        process_names: &processes,
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
fn all_signs_together_add_up_not_mix() {
    let file_list = s(&["danklinux-77.sock", "danklinux-77.pid"]);
    let units = s(&["dms.service"]);
    let processes = s(&["dms", "qs"]);
    let environment = [("DMS_SOCKET", "/run/user/1000/danklinux-77.sock")];
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        active_user_units: &units,
        dbus_names: &[],
        process_names: &processes,
        env: &environment,
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
fn similar_names_do_not_count() {
    // None of these names is a shell.
    let file_list = s(&[
        "noctalia.sock",        // without a display name — not our shape
        "noctalia-wayland-1",   // without an extension
        "danklinux77.sock",     // without a separator
        "danklinux-77.log",     // a foreign extension
        "dbus-1",
    ]);
    let units = s(&["dms.socket", "noctalia-backup.service", "dms-shell.timer"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        active_user_units: &units,
        ..Default::default()
    };
    assert_eq!(detect_shell(&facts), None);
}

#[test]
fn signs_of_both_shells_decided_by_evidence_count() {
    // Leftovers of a past session: one file from Noctalia and the full DMS set.
    let file_list = s(&["noctalia-wayland-1.sock", "danklinux-9.sock"]);
    let units = s(&["dms.service"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &file_list,
        active_user_units: &units,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    // But the main answer does not depend on the choice: the background is foreign in both cases.
    for shell_name in DesktopShell::all() {
        assert!(!wallpaper_layer_owner("niri", "wayland", Some(*shell_name), None).is_ours());
    }
}

// ── owner of the wallpaper layer ───────────────────────────────────────────

#[test]
fn without_shell_background_is_ours() {
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
fn with_shell_background_is_foreign() {
    let o = wallpaper_layer_owner("niri", "wayland", Some(DesktopShell::Noctalia), None);
    assert_eq!(o, WallpaperOwner::Shell(DesktopShell::Noctalia));
    assert!(!o.is_ours());
    assert_eq!(o.shell(), Some(DesktopShell::Noctalia));
    // Noctalia lives on Plasma too — the compositor has nothing to do with it.
    assert!(!wallpaper_layer_owner("kde", "wayland", Some(DesktopShell::Noctalia), None)
        .is_ours());
}

#[test]
fn disabled_shell_wallpaper_returns_background_to_us() {
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
fn on_x11_and_mutter_background_always_ours() {
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
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn foreign_owner_has_message() {
    // The text is in Russian from the real catalog.
    let _lang = i18n_pin::ru();
    for shell_name in DesktopShell::all() {
        let t = shell_owner_message(*shell_name);
        assert!(t.contains("Фон сейчас рисует"), "{t}");
        assert!(t.contains(shell_name.label()), "{t}");
        // What to press — that very button, with the same word as on the button.
        assert!(t.contains("«Отдать обои Rexon»"), "{t}");
        assert!(t.contains("«Wallpaper Engine»"), "{t}");
        assert!(t.contains("примените обои Rexon ещё раз"), "{t}");
        // Jargon (paths, keys, identifiers) has no place in the text.
        for jargon in ["~/", "settings.toml", "screenPreferences", "enabled", "noctalia:"] {
            assert!(!t.contains(jargon), "{jargon} в «{t}»");
        }
    }
    assert_eq!(give_label(), "Отдать обои Rexon");
    assert_eq!(return_label(), "Вернуть оболочке");
}

/// Shell identifiers do not drift apart from the names.
#[test]
fn ids_and_names() {
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
    for shell_name in DesktopShell::all() {
        let t = shell_owner_message(*shell_name);
        assert!(
            !t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
            "{t}"
        );
        assert!(t.starts_with("The background is currently drawn by "), "{t}");
        assert!(t.contains("press “Give Wallpaper to Rexon”"), "{t}");
        assert!(t.ends_with("apply the Rexon wallpaper again."), "{t}");
        assert!(t.contains(shell_name.label()), "{t}");
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
fn noctalia_edit_in_all_file_forms_and_bitwise_restore() {
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
fn noctalia_unsafe_forms_rejected_without_damage() {
    for (text_value, error_text) in [
        ("wallpaper.enabled = true\n", EditError::UnsafeForm),
        ("wallpaper = { enabled = true }\n", EditError::UnsafeForm),
        ("[wallpaper]\nenabled = \"yes\"\n", EditError::UnsafeForm),
        ("wallpaper = 5\n", EditError::UnsafeForm),
        ("[wallpaper\nenabled = true\n", EditError::Unparsable),
    ] {
        assert_eq!(noctalia_disable(Some(text_value)).err(), Some(error_text), "{text_value}");
    }
}

#[test]
fn noctalia_restore_after_shell_rewrote_file() {
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
fn noctalia_settings_layers_and_who_wins() {
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
fn dms_edit_in_all_file_forms_and_bitwise_restore() {
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
fn dms_unsafe_forms_rejected_without_damage() {
    for (text_value, error_text) in [
        ("{\"screenPreferences\": null}", EditError::UnsafeForm),
        ("{\"screenPreferences\": []}", EditError::UnsafeForm),
        ("[1, 2]", EditError::UnsafeForm),
        ("{\"screenPreferences\": {", EditError::Unparsable),
    ] {
        assert_eq!(dms_disable(Some(text_value)).err(), Some(error_text), "{text_value}");
    }
}

#[test]
fn dms_who_draws() {
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
fn on_disk_noctalia_before_after_back_bitwise() {
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
fn on_disk_noctalia_without_state_file_and_xdg_roots() {
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
fn on_disk_dms_before_after_back_bitwise_and_symlink_intact() {
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
fn on_disk_read_only_refused_and_nothing_touched() {
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
fn on_disk_unparsable_file_refused_and_nothing_touched() {
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
fn stale_service_files_filtered_out() {
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
fn strip_only_with_live_shell() {
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
fn session_state_from_files_before_and_after_handover() {
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
fn on_disk_two_shells_notes_do_not_interfere() {
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
