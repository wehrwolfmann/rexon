//! Check of the wallpaper backend choice against how the waywallen daemon
//! itself makes it (0.5.0: `src/plugin/display_registry.rs`,
//! `builtin_display_defs`; `src/wallframe/display/spawner.rs`, `detect_de` +
//! `pick_backend`).
//!
//! The point of the check: if our decision diverges from the daemon, Rexon
//! either starts installing the plugin where the daemon brings up the output
//! itself (two paths fight for the screen), or does not install it where the
//! daemon expects an external one (there will be no wallpaper at all).

use rexon_core::wallpaper_backend as wb;
use rexon_core::wallpaper_backend::WallpaperBackend as B;

/// A Wayland session with the given desktop.
fn wl(desktop: &str) -> B {
    wb::pick(desktop, "wayland", "wayland-0", "")
}

#[test]
fn x11_session_always_goes_to_our_subprocess() {
    // The daemon does not depend on X11, but it has no output client on X11
    // (waywallen-layer-shell is Wayland). Plasma X11 could handle the plugin,
    // but this step was not taken (module header) — for now we draw all X11 ourselves.
    for de in ["XFCE", "MATE", "Budgie", "Pantheon", "X-Cinnamon", "KDE"] {
        assert_eq!(
            wb::pick(de, "x11", "", ""),
            B::X11Subprocess,
            "рабочий стол {de} на X11"
        );
    }
}

/// Pantheon on Wayland («Secure Session»): Gala is a Mutter plugin, it has no
/// wlr-layer-shell (only its own pantheon-desktop-shell protocol) and no shell
/// extensions, so there are no live wallpapers there — an honest Unsupported,
/// not a promise. On X11 («Classic Session») — our process, as everywhere.
#[test]
fn pantheon_wayland_is_honestly_unsupported() {
    assert_eq!(wl("Pantheon"), B::Unsupported);
    assert_eq!(wb::pick("Pantheon", "x11", "", ""), B::X11Subprocess);
}

#[test]
fn kde_is_a_hard_rule_and_never_falls_back() {
    // spawner.rs: «Hard rule: KDE sessions use their dedicated backend
    // and never fall back». Even if XDG_CURRENT_DESKTOP contains something
    // else, KDE wins.
    assert_eq!(wl("KDE"), B::KdePlugin);
    // The daemon recognises KDE only by the `kde` token (`DeCaps::is_kde`).
    assert_eq!(wl("plasma"), B::Unsupported);
    assert_eq!(wl("KDE:wayland"), B::KdePlugin);
    // KDE first, even when a wlr compositor stands next to it.
    assert_eq!(wl("KDE:sway"), B::KdePlugin);
}

#[test]
fn wlr_compositors_are_spawned_by_the_daemon_itself() {
    // The list is the `de` field of the layer-shell definition in
    // builtin_display_defs. river is in the daemon code, although the README
    // table does not have it; wayfire — since 0.5.0.
    for de in ["Hyprland", "sway", "niri", "river", "COSMIC", "Wayfire"] {
        assert_eq!(wl(de), B::DaemonLayerShell, "композитор {de}");
    }
    // Rexon must do NOTHING on these sessions: the daemon itself will find
    // waywallen-layer-shell next to it and start it.
    assert!(!B::DaemonLayerShell.needs_rexon_setup());
}

/// Wayfire does not set `XDG_CURRENT_DESKTOP` itself — the daemon recognises
/// it by `WAYFIRE_SOCKET`, but only when the desktop variable is silent
/// (empty or just `wayland`). Otherwise the desktop is right.
#[test]
fn wayfire_is_recognised_by_its_socket_like_the_daemon_does() {
    let sock = "/run/user/1000/wayfire-wayland-1-.socket";
    let pick = |desktop: &str, socket: &str| wb::pick(desktop, "wayland", "wayland-1", socket);
    assert_eq!(pick("", sock), B::DaemonLayerShell);
    assert_eq!(pick("wayland", sock), B::DaemonLayerShell);
    assert_eq!(pick("Wayland", sock), B::DaemonLayerShell);
    // Without the socket — an honest «not supported», as with the daemon (headless).
    assert_eq!(pick("", ""), B::Unsupported);
    assert_eq!(pick("wayland", ""), B::Unsupported);
    // The desktop is named — the socket changes nothing.
    assert_eq!(pick("KDE", sock), B::KdePlugin);
    assert_eq!(pick("GNOME", sock), B::ShellExtension);
    assert_eq!(pick("weston", sock), B::Unsupported);
    // X11 outweighs everything.
    assert_eq!(wb::pick("", "x11", "", sock), B::X11Subprocess);

    assert_eq!(wb::session_desktops("", sock), "wayfire");
    assert_eq!(wb::session_desktops("wayland", sock), "wayland:wayfire");
    assert_eq!(wb::session_desktops("sway", sock), "sway");
    assert_eq!(wb::session_desktops("wayland", ""), "wayland");
}

/// The list contents are exactly the `de` field of the 0.5.0 daemon's layer-shell definition.
#[test]
fn daemon_layer_shell_list_matches_daemon_0_5_0() {
    assert_eq!(
        wb::DAEMON_LAYER_SHELL_DESKTOPS,
        &["hyprland", "sway", "niri", "river", "cosmic", "wayfire"]
    );
}

#[test]
fn only_kde_requires_setup_from_rexon() {
    // kde-plasma is declared with spawn=External — the daemon expects someone
    // outside to install and select the plugin. This is the only such case.
    assert!(B::KdePlugin.needs_rexon_setup());
    for b in [
        B::DaemonLayerShell,
        B::ShellExtension,
        B::X11Subprocess,
        B::Unsupported,
    ] {
        assert!(!b.needs_rexon_setup(), "{b:?} не требует настройки от нас");
    }
}

#[test]
fn mutter_shells_go_through_the_extension() {
    // Mutter and its fork Muffin do not implement wlr-layer-shell, so we hand
    // the frames to the shell extension via mmap.
    for de in ["GNOME", "ubuntu:GNOME", "Cinnamon", "Unity"] {
        assert_eq!(wl(de), B::ShellExtension, "оболочка {de}");
    }
}

#[test]
fn unknown_wayland_compositor_is_reported_honestly() {
    // In this case the daemon goes headless (PickOutcome::None) — lying to the
    // user that there will be wallpapers is not allowed.
    assert_eq!(wl("weston"), B::Unsupported);
    assert_eq!(wl(""), B::Unsupported);
}

#[test]
fn wayland_detected_by_display_when_session_type_is_missing() {
    // Nested sessions and launching from a TTY: XDG_SESSION_TYPE may be empty,
    // but WAYLAND_DISPLAY is set.
    assert!(wb::is_wayland_session("", "wayland-1"));
    assert!(wb::is_wayland_session("wayland", ""));
    assert!(!wb::is_wayland_session("", ""));
    assert!(!wb::is_wayland_session("x11", ""));
    assert_eq!(wb::pick("Hyprland", "", "wayland-1", ""), B::DaemonLayerShell);
}

#[test]
fn desktop_token_matching_is_case_insensitive_and_split_by_colon() {
    // XDG_CURRENT_DESKTOP can be «pop:GNOME», «KDE», «Hyprland».
    assert_eq!(wl("pop:GNOME"), B::ShellExtension);
    assert_eq!(wl("HYPRLAND"), B::DaemonLayerShell);
    assert_eq!(wl(" niri "), B::DaemonLayerShell);
}

#[test]
fn waywallen_draws_on_kde_and_wlr_only() {
    assert!(B::KdePlugin.is_waywallen());
    assert!(B::DaemonLayerShell.is_waywallen());
    assert!(!B::ShellExtension.is_waywallen());
    assert!(!B::X11Subprocess.is_waywallen());
    assert!(!B::Unsupported.is_waywallen());
}

#[test]
fn every_backend_explains_itself_without_jargon() {
    // Rule #13: we do not show the buyer binary and protocol names.
    for b in [
        B::KdePlugin,
        B::DaemonLayerShell,
        B::ShellExtension,
        B::X11Subprocess,
        B::Unsupported,
    ] {
        let note = b.human_note();
        assert!(!note.is_empty(), "{b:?} без объяснения");
        for jargon in [
            "layer-shell",
            "layer_shell",
            "dmabuf",
            "dma-buf",
            "wlr",
            "kpackage",
            "mmap",
            "containment",
            "waywallen",
        ] {
            assert!(
                !note.to_lowercase().contains(jargon),
                "{b:?}: жаргон «{jargon}» в тексте для пользователя"
            );
        }
        assert!(!backend_id_is_empty(b));
    }
}

fn backend_id_is_empty(b: B) -> bool {
    wb::backend_id(b).is_empty()
}

// ── a display for the daemon when there is none (defect of 30.10.2026, MATE on X11) ──

/// Rexon brings up the display exactly where the daemon cannot itself: KDE
/// (plugin) and X11 (output host client). Every backend is named — a new
/// variant without a decision here will not compile (rule #12).
#[test]
fn display_setup_per_backend() {
    use wb::DisplaySetup as D;
    for b in [B::KdePlugin, B::DaemonLayerShell, B::ShellExtension, B::X11Subprocess, B::Unsupported] {
        let want = match b {
            B::KdePlugin => D::KdePlugin,
            B::X11Subprocess => D::RexonHost,
            B::DaemonLayerShell | B::ShellExtension | B::Unsupported => D::None,
        };
        assert_eq!(b.display_setup(), want, "{b:?}");
        assert_eq!(b.display_setup().rexon_brings_display(), want != D::None, "{b:?}");
    }
    // All X11 desktops listed by rule #12: MATE, Cinnamon X11, XFCE.
    for de in ["MATE", "X-Cinnamon", "XFCE", "Budgie:GNOME", "LXQt"] {
        assert_eq!(wb::pick(de, "x11", "", "").display_setup(), D::RexonHost, "{de}");
    }
}

/// The output definition is read by the daemon as its `DisplayManifest`: a
/// `[display]` section, `spawn = "daemon"`, our host in output client mode,
/// desktops without their own waywallen output — and not a single desktop
/// where the daemon has its own output.
#[test]
fn display_manifest_is_a_daemon_manifest() {
    let text = wb::rexon_display_manifest("/usr/bin/rexon-wallpaper-host");
    let v: toml::Value = text.parse().expect("TOML обязан разбираться");
    let d = v.get("display").expect("секция [display]");
    assert_eq!(d["name"].as_str(), Some(wb::REXON_DISPLAY_NAME));
    assert_eq!(d["bin"].as_str(), Some("/usr/bin/rexon-wallpaper-host"));
    assert_eq!(d["spawn"].as_str(), Some("daemon"));
    let args: Vec<&str> = d["extra_args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(args, ["--mode", "waywallen"]);
    let de: Vec<&str> = d["de"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    for want in ["mate", "xfce", "x-cinnamon"] {
        assert!(de.contains(&want), "{want} в списке столов");
    }
    for own in ["kde", "gnome"].iter().chain(wb::DAEMON_LAYER_SHELL_DESKTOPS) {
        assert!(!de.contains(own), "{own}: у демона свой вывод");
    }
    // Above the daemon's built-in `x11` (50) from waywallen#354: otherwise on
    // MATE, Cinnamon and Xfce it would choose its own output instead of the Rexon host.
    assert!(d["priority"].as_integer().unwrap() > 50, "выше встроенного x11");
    // A path with a quote does not break TOML.
    let odd = wb::rexon_display_manifest("/opt/a\"b/host");
    let v: toml::Value = odd.parse().unwrap();
    assert_eq!(v["display"]["bin"].as_str(), Some("/opt/a\"b/host"));
}

#[test]
fn display_manifest_paths_and_refresh() {
    assert_eq!(
        wb::rexon_display_manifest_path("/home/u/.local/share/"),
        "/home/u/.local/share/waywallen/displays/rexon-x11.toml"
    );
    assert_eq!(wb::rexon_host_lock_path("/run/user/1000"), "/run/user/1000/rexon-waywallen-display.lock");
    let host = "/usr/bin/rexon-wallpaper-host";
    assert!(wb::rexon_display_manifest_stale(None, host), "нет файла — писать");
    let cur = wb::rexon_display_manifest(host);
    assert!(!wb::rexon_display_manifest_stale(Some(&cur), host), "тот же — не трогать");
    assert!(wb::rexon_display_manifest_stale(Some(&cur), "/opt/rexon/host"), "хост переехал — переписать");
}

/// On Wayland and without `DISPLAY` the output host client only waits.
#[test]
fn display_host_idles_off_x11() {
    assert!(!wb::rexon_host_should_idle("x11", "", ":0"));
    assert!(wb::rexon_host_should_idle("wayland", "wayland-0", ":0"), "Cinnamon/XFCE на Wayland");
    assert!(wb::rexon_host_should_idle("", "wayland-1", ""));
    assert!(wb::rexon_host_should_idle("x11", "", ""), "нет DISPLAY");
}

/// Rexon starts the host with the same line as the daemon does from the output definition.
#[test]
fn display_host_argv_matches_daemon_spawn() {
    assert_eq!(
        wb::rexon_display_host_argv("/usr/bin/rexon-wallpaper-host", "/run/user/1000/waywallen/display.sock"),
        ["/usr/bin/rexon-wallpaper-host", "--socket", "/run/user/1000/waywallen/display.sock", "--mode", "waywallen"]
    );
    assert_eq!(wb::rexon_display_host_argv("/h", ""), ["/h", "--mode", "waywallen"]);
    assert_eq!(
        wb::rexon_display_host_sibling("/r/target-shared/release/rexon-main-window").as_deref(),
        Some("/r/target-shared/release/rexon-wallpaper-host")
    );
}

// ── desktop icons under live wallpapers (X11) ─────────────────────────

use std::collections::HashMap;
use wb::DesktopIconsOwner as Owner;

/// A mock desktop: gsettings/xfconf values, a write log and state.
#[derive(Default)]
struct FakeDesk {
    values: HashMap<String, String>,
    caja_keys: Vec<&'static str>,
    /// Schema defaults for `gsettings reset` (none — the key is empty after the reset).
    defaults: HashMap<String, String>,
    /// Keys of the org.nemo.desktop schema; empty — Nemo is not installed.
    nemo_keys: Vec<&'static str>,
    state: Option<String>,
    sets: Vec<String>,
    pauses: Vec<u64>,
    /// gsettings set refuses.
    set_fails: bool,
}

impl FakeDesk {
    fn mate(icons: bool, has_key: bool) -> Self {
        let mut d = FakeDesk::default();
        d.put("org.mate.background show-desktop-icons", if icons { "true" } else { "false" });
        d.put("org.mate.session.required-components windowmanager", "'marco'");
        d.put("org.mate.Marco.general compositing-manager", "true");
        d.caja_keys = vec!["font", "home-icon-visible"];
        if has_key {
            d.caja_keys.push("transparent-background");
            d.put("org.mate.caja.desktop transparent-background", "false");
        }
        d
    }
    fn put(&mut self, k: &str, v: &str) {
        self.values.insert(k.to_string(), v.to_string());
    }
    fn get(&self, k: &str) -> &str {
        self.values.get(k).map(String::as_str).unwrap_or("")
    }
}

impl wb::DesktopIconsIo for FakeDesk {
    fn run(&mut self, argv: &[&str]) -> Option<String> {
        match argv {
            ["gsettings", "get", schema, key] => self.values.get(&format!("{schema} {key}")).map(|v| format!("{v}\n")),
            ["gsettings", "list-keys", "org.mate.caja.desktop"] => Some(self.caja_keys.join("\n")),
            ["gsettings", "list-keys", "org.nemo.desktop"] => {
                (!self.nemo_keys.is_empty()).then(|| self.nemo_keys.join("\n"))
            }
            ["gsettings", "set", schema, key, value] => {
                self.sets.push(format!("{schema} {key}={value}"));
                if self.set_fails {
                    return None;
                }
                self.put(&format!("{schema} {key}"), value);
                Some(String::new())
            }
            ["gsettings", "reset", schema, key] => {
                let k = format!("{schema} {key}");
                self.sets.push(format!("{k} reset"));
                match self.defaults.get(&k).cloned() {
                    Some(v) => self.values.insert(k, v),
                    None => self.values.remove(&k),
                };
                Some(String::new())
            }
            ["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/style"] => {
                self.values.get("xfce style").cloned()
            }
            other => panic!("неожиданная команда {other:?}"),
        }
    }
    fn read_state(&mut self) -> Option<String> {
        self.state.clone()
    }
    fn write_state(&mut self, content: Option<&str>) -> bool {
        self.state = content.map(str::to_string);
        true
    }
    fn pause_ms(&mut self, ms: u64) {
        self.pauses.push(ms);
    }
}

#[test]
fn desktop_icons_owner_only_on_x11_desktops_with_icon_windows() {
    assert_eq!(wb::desktop_icons_owner("mate", "x11"), Some(Owner::Caja));
    assert_eq!(wb::desktop_icons_owner("cinnamon", "x11"), Some(Owner::Nemo));
    assert_eq!(wb::desktop_icons_owner("xfce", "x11"), Some(Owner::Xfdesktop));
    assert_eq!(wb::desktop_icons_owner("mate", "wayland"), None);
    assert_eq!(wb::desktop_icons_owner("kde", "x11"), None);
    assert_eq!(
        wb::desktop_icons_state_path("", "/home/w"),
        "/home/w/.local/state/rexon/desktop-icons.state"
    );
    assert_eq!(wb::desktop_icons_state_path("/s/", "/home/w"), "/s/rexon/desktop-icons.state");
}

/// Caja without the key (MATE 1.28 out of the box): an honest warning, the
/// icons are NOT turned off, nothing is written.
#[test]
fn caja_without_key_warns_and_touches_nothing() {
    let mut d = FakeDesk::mate(true, false);
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(notice, Some(wb::desktop_icons_hidden_text(Owner::Caja)));
    assert!(d.sets.is_empty(), "значки и настройки не трогаются: {:?}", d.sets);
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d), "откатывать нечего");
}

/// Icons turned off by the person — stay silent and do nothing (key or no key).
#[test]
fn icons_off_means_nothing_to_say_or_do() {
    for has_key in [false, true] {
        let mut d = FakeDesk::mate(false, has_key);
        assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), None);
        assert!(d.sets.is_empty());
    }
}

/// Caja with the key: the key is turned on, the desktop window is recreated
/// by toggling the icons (not `caja -q`), the former value is recorded; a
/// repeat touches nothing; rollback restores the key and recreates the window.
#[test]
fn caja_with_key_turns_on_see_through_and_restores() {
    let mut d = FakeDesk::mate(true, true);
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(notice, Some(wb::desktop_icons_kept_text(false)));
    assert_eq!(
        d.sets,
        [
            "org.mate.caja.desktop transparent-background=true",
            "org.mate.background show-desktop-icons=false",
            "org.mate.background show-desktop-icons=true",
        ]
    );
    assert_eq!(d.pauses, [wb::CAJA_DESKTOP_RECREATE_PAUSE_MS]);
    assert_eq!(d.get("org.mate.background show-desktop-icons"), "true", "значки остались включены");
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert!(saved.caja_transparent_was_off && !saved.marco_compositing_was_off);

    // The second apply: everything is already in place — silently.
    d.sets.clear();
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), None);
    assert!(d.sets.is_empty());

    // «Stop live wallpapers» — as it was.
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(
        d.sets,
        [
            "org.mate.caja.desktop transparent-background=false",
            "org.mate.background show-desktop-icons=false",
            "org.mate.background show-desktop-icons=true",
        ]
    );
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d), "второй откат — пустой");
}

/// The Marco compositor is off — it is turned on and restored; the former
/// value is recorded only by the first apply.
#[test]
fn marco_compositing_turned_on_and_back() {
    let mut d = FakeDesk::mate(true, true);
    d.put("org.mate.Marco.general compositing-manager", "false");
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja).unwrap();
    assert_eq!(notice, wb::desktop_icons_kept_text(true));
    assert_eq!(d.sets[0], "org.mate.Marco.general compositing-manager=true");
    let first_state = d.state.clone();
    // The person turned off the compositor themselves and applies again — the former value is not overwritten.
    d.put("org.mate.Marco.general compositing-manager", "false");
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(d.state, first_state);
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.contains(&"org.mate.Marco.general compositing-manager=false".to_string()), "{:?}", d.sets);
    assert!(d.sets.contains(&"org.mate.caja.desktop transparent-background=false".to_string()));
}

/// Not Marco (e.g. Compiz) — the compositor is not touched.
#[test]
fn other_window_manager_compositor_is_left_alone() {
    let mut d = FakeDesk::mate(true, true);
    d.put("org.mate.session.required-components windowmanager", "'compiz'");
    d.put("org.mate.Marco.general compositing-manager", "false");
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert!(!d.sets.iter().any(|s| s.contains("Marco")), "{:?}", d.sets);
}

/// gsettings does not write — an honest «icons hidden», not «remain».
#[test]
fn failed_switch_falls_back_to_warning() {
    let mut d = FakeDesk::mate(true, true);
    d.set_fails = true;
    assert_eq!(
        wb::prepare_desktop_icons(&mut d, Owner::Caja),
        Some(wb::desktop_icons_hidden_text(Owner::Caja))
    );
}

/// Rollback when the person already turned the key off — the desktop window is not poked.
#[test]
fn restore_skips_key_already_off() {
    let mut d = FakeDesk::mate(true, true);
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    d.put("org.mate.caja.desktop transparent-background", "false");
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.is_empty(), "{:?}", d.sets);
}

/// xfdesktop: icons present (style not 0 or not set) — a warning; style 0 —
/// stay silent. Nemo is always transparent.
#[test]
fn xfdesktop_and_nemo() {
    let mut d = FakeDesk::default();
    assert_eq!(
        wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop),
        Some(wb::desktop_icons_hidden_text(Owner::Xfdesktop)),
        "не задано — умолчание xfdesktop со значками"
    );
    d.put("xfce style", "2\n");
    assert!(wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop).is_some());
    d.put("xfce style", "0\n");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop), None);
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Nemo), None);
    assert!(d.sets.is_empty());
}

#[test]
fn saved_state_round_trips_and_rejects_foreign_text() {
    let saved = wb::DesktopIconsSaved {
        caja_recorded: true,
        caja_transparent_was_off: true,
        marco_compositing_was_off: false,
        nemo_ignored_before: None,
    };
    assert_eq!(wb::DesktopIconsSaved::parse(&saved.to_text()), Some(saved));
    let nemo = wb::DesktopIconsSaved {
        nemo_ignored_before: Some("['conky', 'csd-background']".into()),
        ..Default::default()
    };
    assert_eq!(wb::DesktopIconsSaved::parse(&nemo.to_text()), Some(nemo));
    // A file of the former version (Caja only) is read as it was.
    let old = wb::DesktopIconsSaved::parse("owner=caja\ncaja-transparent-was-off=true\nmarco-compositing-was-off=false\n")
        .unwrap();
    assert!(old.caja_recorded && old.caja_transparent_was_off && old.nemo_ignored_before.is_none());
    assert_eq!(wb::DesktopIconsSaved::parse("мусор"), None);
    assert_eq!(wb::DesktopIconsSaved::parse("nemo-ignored-desktop-handlers-before=не массив"), None);
    let mut d = FakeDesk::mate(true, true);
    d.state = Some("мусор".into());
    assert!(!wb::restore_desktop_icons(&mut d));
    assert_eq!(d.state, None, "битый файл убран");
}

// ── nemo-desktop: the wallpaper window is not a «foreign desktop» ─────────────────────────────

const NEMO_KEY: &str = "org.nemo.desktop ignored-desktop-handlers";

fn with_nemo(mut d: FakeDesk, value: &str) -> FakeDesk {
    d.nemo_keys = vec!["font", "ignored-desktop-handlers", "show-desktop-icons"];
    d.put(NEMO_KEY, value);
    d.defaults.insert(NEMO_KEY.into(), "['conky', 'csd-background']".into());
    d
}

/// Defect of 31.10.2026 (Nemo stand): nemo-desktop started after the wallpaper
/// host exited with «Desktop already managed». Rexon appends `rexon` to the
/// foreign values, a repeat touches nothing, «Stop» restores the former value
/// verbatim.
#[test]
fn nemo_ignored_handlers_get_rexon_and_come_back() {
    let mut d = with_nemo(FakeDesk::default(), "['conky', 'csd-background']");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Nemo), None, "Nemo прозрачен — сказать нечего");
    assert_eq!(d.sets, [format!("{NEMO_KEY}=['conky', 'csd-background', 'rexon']")]);
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert_eq!(saved.nemo_ignored_before.as_deref(), Some("['conky', 'csd-background']"));
    assert!(!saved.caja_recorded);

    d.sets.clear();
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty(), "уже дописано: {:?}", d.sets);

    // The former value matches the schema default — a reset, without an explicit write.
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(d.sets, [format!("{NEMO_KEY} reset")]);
    assert_eq!(d.get(NEMO_KEY), "['conky', 'csd-background']");
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d));
}

/// The person edited the list while the wallpapers ran — rollback removes only ours.
#[test]
fn nemo_restore_keeps_foreign_edits() {
    let mut d = with_nemo(FakeDesk::default(), "@as []");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert_eq!(d.get(NEMO_KEY), "['rexon']");
    d.put(NEMO_KEY, "['rexon', 'my-dock']");
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(d.sets, [format!("{NEMO_KEY}=['my-dock']")]);

    // An empty former list (not the default) — the reset did not match, it is written explicitly.
    let mut d = with_nemo(FakeDesk::default(), "@as []");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    d.sets.clear();
    wb::restore_desktop_icons(&mut d);
    assert_eq!(d.sets, [format!("{NEMO_KEY} reset"), format!("{NEMO_KEY}=@as []")]);
    assert_eq!(d.get(NEMO_KEY), "@as []");
}

/// `rexon` is already in the list (the person added it themselves, any case)
/// — not touched and not rolled back; no Nemo schema — not touched at all.
#[test]
fn nemo_left_alone_when_already_ignored_or_absent() {
    let mut d = with_nemo(FakeDesk::default(), "['conky', 'Rexon']");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty());
    assert_eq!(d.state, None);

    let mut d = FakeDesk::default();
    d.put(NEMO_KEY, "['conky']");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty(), "схемы нет — {:?}", d.sets);

    // The value cannot be parsed — write nothing (do not overwrite foreign data).
    let mut d = with_nemo(FakeDesk::default(), "<странное>");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty());
    assert_eq!(d.state, None);
}

/// nemo-desktop outside Cinnamon (stand: on MATE with Caja): both parts are
/// written to one file, neither overwrites the other; rollback restores both.
#[test]
fn nemo_and_caja_share_one_state_file() {
    let mut d = with_nemo(FakeDesk::mate(true, true), "['conky', 'csd-background']");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), Some(wb::desktop_icons_kept_text(false)));
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert!(saved.caja_recorded && saved.caja_transparent_was_off, "Caja записана после Nemo: {saved:?}");
    assert_eq!(saved.nemo_ignored_before.as_deref(), Some("['conky', 'csd-background']"));
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.contains(&"org.mate.caja.desktop transparent-background=false".to_string()), "{:?}", d.sets);
    assert!(d.sets.contains(&format!("{NEMO_KEY} reset")), "{:?}", d.sets);
    assert_eq!(d.get(NEMO_KEY), "['conky', 'csd-background']");
    assert_eq!(d.state, None);
}

#[test]
fn gvariant_string_arrays_round_trip() {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(wb::parse_gvariant_strv("['conky', 'csd-background']"), Some(v(&["conky", "csd-background"])));
    assert_eq!(wb::parse_gvariant_strv("@as []"), Some(vec![]));
    assert_eq!(wb::parse_gvariant_strv("[]\n"), Some(vec![]));
    assert_eq!(wb::parse_gvariant_strv(r#"["it's", 'a\'b']"#), Some(v(&["it's", "a'b"])));
    assert_eq!(wb::parse_gvariant_strv("conky"), None);
    assert_eq!(wb::parse_gvariant_strv("['open"), None);
    for items in [v(&[]), v(&["a"]), v(&["it's", "back\\slash", "rexon"])] {
        assert_eq!(wb::parse_gvariant_strv(&wb::format_gvariant_strv(&items)), Some(items.clone()));
    }
    assert_eq!(wb::format_gvariant_strv(&v(&["conky", "rexon"])), "['conky', 'rexon']");
}

/// Which icon windows are recreated by toggling the icons (by `WM_CLASS`, as
/// on the xfwm4 stand of 31.10.2026).
#[test]
fn icons_window_toggle_by_wm_class() {
    assert_eq!(
        wb::icons_window_toggle("nemo-desktop", "Nemo-desktop"),
        Some(("org.nemo.desktop", "show-desktop-icons"))
    );
    assert_eq!(
        wb::icons_window_toggle("desktop_window", "Caja"),
        Some(("org.mate.background", "show-desktop-icons"))
    );
    assert_eq!(wb::icons_window_toggle("caja", "Caja"), None, "обычное окно Caja");
    assert_eq!(wb::icons_window_toggle("xfdesktop", "Xfdesktop"), None, "фон непрозрачный — не поможет");
}

/// Recreating the icon window: «off → pause → on»; the key is off — nothing;
/// «on» failed — another attempt, the icons do not stay off.
#[test]
fn recreate_icons_window_toggles_and_restores() {
    let mut d = FakeDesk::default();
    d.put("org.nemo.desktop show-desktop-icons", "true");
    assert!(wb::recreate_icons_window(&mut d, "org.nemo.desktop", "show-desktop-icons"));
    assert_eq!(d.sets, ["org.nemo.desktop show-desktop-icons=false", "org.nemo.desktop show-desktop-icons=true"]);
    assert_eq!(d.pauses, [wb::CAJA_DESKTOP_RECREATE_PAUSE_MS]);
    assert_eq!(d.get("org.nemo.desktop show-desktop-icons"), "true");

    let mut off = FakeDesk::default();
    off.put("org.nemo.desktop show-desktop-icons", "false");
    assert!(!wb::recreate_icons_window(&mut off, "org.nemo.desktop", "show-desktop-icons"));
    assert!(off.sets.is_empty(), "значки выключены человеком — не трогаем");

    let mut none = FakeDesk::default();
    assert!(!wb::recreate_icons_window(&mut none, "org.nemo.desktop", "show-desktop-icons"), "нет схемы");
    assert!(none.sets.is_empty());

    let mut broken = FakeDesk::default();
    broken.put("org.mate.background show-desktop-icons", "true");
    broken.set_fails = true;
    assert!(!wb::recreate_icons_window(&mut broken, "org.mate.background", "show-desktop-icons"));
    assert_eq!(broken.sets, ["org.mate.background show-desktop-icons=false"], "«выкл» не прошло — дальше не идём");
}

// ── GNOME: the waywallen output extension (defect of 09.11.2026, black desktop) ──

/// A mock session: environment, directories and command answers by the first word.
struct GnomeFake {
    env: std::collections::BTreeMap<&'static str, &'static str>,
    dirs: Vec<String>,
    /// The `GetExtensionInfo` answer; `None` — the shell did not answer.
    info: Option<String>,
    /// The shell's answer to Enable/DisableExtension; `None` — the bus is silent.
    toggle: Option<&'static str>,
    /// Whether the `sh -c` fallback script succeeded.
    script_ok: bool,
    ran: std::cell::RefCell<Vec<Vec<String>>>,
}

impl GnomeFake {
    fn new(desktop: &'static str, state: Option<f64>) -> GnomeFake {
        GnomeFake {
            env: [("XDG_CURRENT_DESKTOP", desktop), ("HOME", "/home/u")].into_iter().collect(),
            dirs: vec![format!("/home/u/.local/share/gnome-shell/extensions/{}", wb::WAYWALLEN_GNOME_EXT_ID)],
            info: state.map(|st| {
                format!("({{'uuid': <'{}'>, 'state': <{st:.1}>}},)", wb::WAYWALLEN_GNOME_EXT_ID)
            }),
            toggle: Some("(true,)"),
            script_ok: true,
            ran: Default::default(),
        }
    }
    fn scripts(&self) -> Vec<String> {
        self.ran.borrow().iter().filter(|a| a[0] == "sh").map(|a| a[2].clone()).collect()
    }
    /// Shell methods that were called, except the state query.
    fn toggles(&self) -> Vec<String> {
        self.ran
            .borrow()
            .iter()
            .filter(|a| a[0] == "gdbus" && !a[8].ends_with("GetExtensionInfo"))
            .map(|a| a[8].rsplit('.').next().unwrap().to_string())
            .collect()
    }
}

impl wb::GnomeExtIo for GnomeFake {
    fn env(&self, name: &str) -> String {
        self.env.get(name).copied().unwrap_or_default().to_string()
    }
    fn is_dir(&self, path: &str) -> bool {
        self.dirs.iter().any(|d| d == path)
    }
    fn run(&self, argv: &[String]) -> Option<String> {
        self.ran.borrow_mut().push(argv.to_vec());
        match argv[0].as_str() {
            "gdbus" => {
                assert_eq!(argv.last().map(String::as_str), Some(wb::WAYWALLEN_GNOME_EXT_ID));
                if argv[8].ends_with("GetExtensionInfo") {
                    self.info.clone()
                } else {
                    self.toggle.map(str::to_string)
                }
            }
            "sh" => self.script_ok.then(String::new),
            other => panic!("неожиданная команда {other}"),
        }
    }
}

#[test]
fn waywallen_gnome_display_switches_only_what_is_needed() {
    use wb::GnomeDisplaySwitch as R;
    // Enabled → the shell itself disables it; no list edit of our own.
    let io = GnomeFake::new("GNOME", Some(1.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
    assert_eq!(io.toggles(), vec!["DisableExtension"]);
    assert!(io.scripts().is_empty(), "{:?}", io.scripts());
    // Enabled and asked to enable — we call nothing.
    let io = GnomeFake::new("GNOME", Some(1.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Already);
    assert!(io.toggles().is_empty() && io.scripts().is_empty());
    // Disabled → enable.
    let io = GnomeFake::new("ubuntu:GNOME", Some(2.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Switched);
    assert_eq!(io.toggles(), vec!["EnableExtension"]);
    // Disabled and asked to disable; loaded but never enabled (6); error; wrong
    // version.
    for st in [2.0, 6.0, 3.0, 4.0] {
        let io = GnomeFake::new("GNOME", Some(st));
        assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Already, "{st}");
        assert!(io.toggles().is_empty() && io.scripts().is_empty(), "{st}");
    }
    // An error in the extension: we enable it, but there will be no display — the hold does not wait.
    let io = GnomeFake::new("GNOME", Some(3.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Failed);
    // The bus did not answer the toggle — the fallback path: the settings list
    // without foreign uuids.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.toggle = None;
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
    let s = io.scripts();
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0].contains("grep -vF 'org.waywallen.gnome@waywallen.io'"), "{}", s[0]);
    assert!(s[0].contains("gsettings set org.gnome.shell enabled-extensions"), "{}", s[0]);
    let mut io = GnomeFake::new("GNOME", Some(2.0));
    io.toggle = Some("(false,)");
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Switched);
    assert!(io.scripts()[0].contains("'org.waywallen.gnome@waywallen.io'"), "{:?}", io.scripts());
    // And the fallback path failed too.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.toggle = None;
    io.script_ok = false;
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Failed);
}

#[test]
fn waywallen_gnome_display_skips_foreign_sessions() {
    use wb::GnomeDisplaySwitch as R;
    // Not GNOME Shell — we do not even ask.
    for de in ["KDE", "X-Cinnamon", "Budgie:GNOME", "Unity", "Pantheon:GNOME", "Hyprland", ""] {
        let io = GnomeFake::new(de, Some(1.0));
        assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped, "{de}");
        assert!(io.ran.borrow().is_empty(), "{de}: {:?}", io.ran.borrow());
    }
    // No extension — likewise.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.dirs.clear();
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped);
    assert!(io.ran.borrow().is_empty());
    // The shell did not answer (no gnome-shell on the bus).
    let io = GnomeFake::new("GNOME", None);
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped);
    assert!(io.scripts().is_empty());
    // The system directory from XDG_DATA_DIRS is found the same way.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.dirs = vec![format!("/opt/share/gnome-shell/extensions/{}", wb::WAYWALLEN_GNOME_EXT_ID)];
    io.env.insert("XDG_DATA_DIRS", "/opt/share/:/usr/share");
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
}

#[test]
fn gnome_display_setup_needs_installed_waywallen_extension() {
    use wb::DisplaySetup as D;
    let gnome = wb::pick("GNOME", "wayland", "wayland-0", "");
    assert_eq!(wb::display_setup_for(gnome, "GNOME", true), D::GnomeExtension);
    assert!(D::GnomeExtension.rexon_brings_display());
    assert_eq!(wb::display_setup_for(gnome, "GNOME", false), D::None);
    // Cinnamon on Wayland — the same backend, but the waywallen extension is not its own.
    let cin = wb::pick("X-Cinnamon", "wayland", "wayland-0", "");
    assert_eq!(wb::display_setup_for(cin, "X-Cinnamon", true), D::None);
    // The other backends — as without the extension.
    for de in ["KDE", "Hyprland"] {
        let b = wb::pick(de, "wayland", "wayland-0", "");
        assert_eq!(wb::display_setup_for(b, de, true), b.display_setup(), "{de}");
    }
    assert_eq!(wb::display_setup_for(wb::pick("GNOME", "x11", "", ""), "GNOME", true), D::RexonHost);
}

// ── Cinnamon with layer-shell (Muffin 6.8+) ────────────────────────────────

/// The sign is the compositor protocol; without a compositor answer — the
/// background service. The version number is not looked at; outside Cinnamon
/// on Wayland — always no.
#[test]
fn cinnamon_layer_shell_by_protocol_then_service() {
    let ls = vec!["wl_compositor".to_string(), wb::LAYER_SHELL_GLOBAL.to_string()];
    let plain = vec!["wl_compositor".to_string()];
    assert!(wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", Some(&ls), false));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", Some(&plain), true), "протокол главнее службы");
    assert!(wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", None, true));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", None, false));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "x11", "", Some(&ls), true), "X11 — прежний путь окном");
    assert!(!wb::cinnamon_layer_shell("GNOME", "wayland", "wayland-0", Some(&ls), true));
}

#[test]
fn cinnamon_bg_service_files_follow_xdg_order() {
    let files = wb::cinnamon_bg_service_files("", "/home/u", "/usr/local/share:/usr/share/");
    assert_eq!(
        files,
        vec![
            "/home/u/.local/share/dbus-1/services/org.Cinnamon.Background.service",
            "/usr/local/share/dbus-1/services/org.Cinnamon.Background.service",
            "/usr/share/dbus-1/services/org.Cinnamon.Background.service",
        ]
    );
}

/// Cinnamon with layer-shell: scenes are output by the Rexon host (it brings
/// up the regular layer-shell client), not by the shell extension — «Apply» is enabled.
#[test]
fn cinnamon_layer_shell_display_is_rexon_host() {
    use wb::{DisplaySetup, WallpaperBackend as B};
    assert_eq!(wb::display_setup_in(B::ShellExtension, "X-Cinnamon", false, true), DisplaySetup::RexonHost);
    assert_eq!(wb::display_setup_in(B::ShellExtension, "X-Cinnamon", false, false), DisplaySetup::None);
    assert_eq!(wb::display_setup_in(B::ShellExtension, "GNOME", true, true), DisplaySetup::GnomeExtension, "GNOME не трогаем");
    assert_eq!(wb::display_setup_in(B::X11Subprocess, "X-Cinnamon", false, true), DisplaySetup::RexonHost);
}

/// The host on Wayland: layer-shell and a found client — bring up the client;
/// otherwise wait. The daemon socket is passed as is, empty — the client finds it itself.
#[test]
fn wayland_host_delegates_only_with_layer_shell_and_client() {
    let ls = vec![wb::LAYER_SHELL_GLOBAL.to_string()];
    assert_eq!(
        wb::wayland_host_role(&ls, Some("/opt/ww/waywallen-layer-shell"), "/run/user/1/waywallen/display.sock"),
        wb::WaylandHostRole::Delegate(vec![
            "/opt/ww/waywallen-layer-shell".to_string(),
            "--socket".to_string(),
            "/run/user/1/waywallen/display.sock".to_string(),
            "--name".to_string(),
            "rexon".to_string(),
        ])
    );
    assert_eq!(
        wb::wayland_host_role(&ls, Some("/b/waywallen-layer-shell"), ""),
        wb::WaylandHostRole::Delegate(vec!["/b/waywallen-layer-shell".into(), "--name".into(), "rexon".into()])
    );
    assert_eq!(wb::wayland_host_role(&ls, None, ""), wb::WaylandHostRole::Idle);
    assert_eq!(wb::wayland_host_role(&[], Some("/b/waywallen-layer-shell"), ""), wb::WaylandHostRole::Idle);
}

/// The output client is looked for next to the parent daemon (AppImage), then
/// next to the package daemon; a foreign parent (not the daemon) does not count.
#[test]
fn layer_shell_client_found_next_to_daemon() {
    let c = wb::layer_shell_client_candidates(Some("/tmp/.mount_ww/usr/bin/waywallen"), &[]);
    assert_eq!(c[0], "/tmp/.mount_ww/usr/bin/waywallen-layer-shell");
    let c = wb::layer_shell_client_candidates(Some("/usr/bin/bash"), &[]);
    assert!(!c.iter().any(|p| p.starts_with("/usr/bin/bash")), "{c:?}");
    assert!(c.iter().all(|p| p.ends_with("/waywallen-layer-shell")));
}

#[test]
fn layer_shell_restart_backs_off_with_ceiling() {
    assert_eq!(wb::layer_shell_restart_delay_ms(0), 2000);
    assert_eq!(wb::layer_shell_restart_delay_ms(1), 4000);
    assert_eq!(wb::layer_shell_restart_delay_ms(3), 16_000);
    assert_eq!(wb::layer_shell_restart_delay_ms(9), 30_000);
}

#[test]
fn display_off_marker_lives_in_state_dir() {
    assert_eq!(wb::display_off_marker_path("", "/home/u"), "/home/u/.local/state/rexon/waywallen-display.off");
    assert_eq!(wb::display_off_marker_path("/s/", "/home/u"), "/s/rexon/waywallen-display.off");
}

#[test]
fn gdbus_answers_are_parsed() {
    assert!(wb::parse_gdbus_bool("(true,)\n"));
    assert!(!wb::parse_gdbus_bool("(false,)\n"));
    assert_eq!(wb::parse_gdbus_uint("(uint32 4321,)\n"), Some(4321));
    assert_eq!(wb::parse_gdbus_uint("()"), None);
}

/// A mock bus and systemd for restacking the background.
struct BgIo {
    owner: std::cell::RefCell<Vec<bool>>,
    unit_active: bool,
    restart_ok: bool,
    ran: std::cell::RefCell<Vec<String>>,
}

impl BgIo {
    fn new(owner: &[bool], unit_active: bool, restart_ok: bool) -> Self {
        BgIo { owner: std::cell::RefCell::new(owner.to_vec()), unit_active, restart_ok, ran: Default::default() }
    }
}

impl wb::CinnamonBgIo for BgIo {
    fn run(&self, argv: &[String]) -> Option<String> {
        let line = argv.join(" ");
        self.ran.borrow_mut().push(line.clone());
        if line.contains("NameHasOwner") {
            let mut o = self.owner.borrow_mut();
            let v = if o.len() > 1 { o.remove(0) } else { o[0] };
            return Some(format!("({v},)\n"));
        }
        if line.contains("is-active") {
            return self.unit_active.then(|| "active\n".to_string());
        }
        if line.contains("systemctl --user restart") {
            return self.restart_ok.then(String::new);
        }
        if line.contains("GetConnectionUnixProcessID") {
            return Some("(uint32 777,)\n".to_string());
        }
        Some(String::new())
    }
    fn sleep_ms(&self, _ms: u64) {}
}

/// No service — touch nothing; there is a systemd unit — `restart` (the limit
/// of the Cinnamon background manager does not bother us); without systemd — a
/// signal to the name owner and our own `Start()` (D-Bus activation), we wait
/// for the name to return.
#[test]
fn restack_restarts_background_service_safely() {
    let io = BgIo::new(&[false], true, true);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::NotRunning);
    assert!(!io.ran.borrow().iter().any(|l| l.contains("restart") || l.contains("kill")));

    let io = BgIo::new(&[true], true, true);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Restarted);
    assert!(io.ran.borrow().iter().any(|l| l == "systemctl --user restart cinnamon-background-daemon.service"));
    assert!(!io.ran.borrow().iter().any(|l| l.starts_with("kill")));

    let io = BgIo::new(&[true], true, false);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Failed);

    let io = BgIo::new(&[true, false, true], false, false);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Restarted);
    let ran = io.ran.borrow();
    let kill = ran.iter().position(|l| l == "kill -TERM 777").expect("сигнал владельцу имени");
    let start = ran.iter().position(|l| l.contains("org.Cinnamon.Background.Start")).expect("свой Start()");
    assert!(kill < start);
}

/// Protection against a restart loop: no more often than the limit per
/// window; the bus watcher does not count our own restart (a fresh stamp) as
/// a service crash; a restack already done by another Rexon process is not repeated.
#[test]
fn restack_guards_against_loops() {
    let now = 1_000_000;
    assert!(wb::bg_restack_allowed(&[], now));
    let full: Vec<u64> = (0..wb::BG_RESTACK_LIMIT as u64).map(|i| now - 1000 * i).collect();
    assert!(!wb::bg_restack_allowed(&full, now));
    let old: Vec<u64> = full.iter().map(|t| t - wb::BG_RESTACK_WINDOW_MS).collect();
    assert!(wb::bg_restack_allowed(&old, now), "старые перезапуски не в счёт");

    assert!(wb::bg_owner_change_is_ours(Some(now - 1000), now));
    assert!(!wb::bg_owner_change_is_ours(Some(now - wb::BG_OWN_RESTART_WINDOW_MS - 1), now));
    assert!(!wb::bg_owner_change_is_ours(None, now));

    assert!(wb::bg_restack_done_by_other(Some(now + 3000), now, 2500));
    assert!(!wb::bg_restack_done_by_other(Some(now + 1000), now, 2500), "раньше нашей поверхности — не в счёт");
    assert!(!wb::bg_restack_done_by_other(None, now, 2500));
    assert_eq!(wb::bg_restack_stamp_path("/run/user/1/"), "/run/user/1/rexon-cinnamon-bg-restack.stamp");
    assert_eq!(wb::bg_restack_lock_path("/run/user/1"), "/run/user/1/rexon-cinnamon-bg-restack.stamp.lock");
}

/// The stamp remembers the start and end of the restart: «already done by
/// another» is judged by the START (a restart begun before our surface was
/// shown did not cover it, even if it ended after), «is it our restart» for
/// the bus watcher — by the last moment. A stamp of the former kind (one
/// number) is read as the start.
#[test]
fn restack_stamp_keeps_start_and_end() {
    let s = wb::BgRestackStamp { started: 100, finished: Some(250) };
    assert_eq!(wb::parse_bg_restack_stamp(&s.to_text()), Some(s));
    assert_eq!(s.latest(), 250);
    let running = wb::BgRestackStamp { started: 100, finished: None };
    assert_eq!(wb::parse_bg_restack_stamp("100\n"), Some(running));
    assert_eq!(running.latest(), 100);
    assert_eq!(wb::parse_bg_restack_stamp(""), None);
    assert_eq!(wb::parse_bg_restack_stamp("мусор"), None);
    // Shown at 200: a restart 100→250 did not cover it, 210→300 did.
    assert!(!wb::bg_restack_done_by_other(Some(100), 200, 0));
    assert!(wb::bg_restack_done_by_other(Some(210), 200, 0));
}

/// Orders merge: deadline requests only postpone the execution and remember
/// the first order; the news «first frame on screen» pulls the deadline to
/// almost-now and itself becomes the reference point — without a margin.
#[test]
fn restack_orders_merge_and_first_frame_pulls_in() {
    let fallback = wb::merge_bg_restack_order(None, 1000, wb::BG_RESTACK_FALLBACK_MS, false);
    assert_eq!(fallback, wb::BgRestackOrder { due: 1000 + wb::BG_RESTACK_FALLBACK_MS, requested: 1000, slack: wb::BG_RESTACK_FALLBACK_MS });
    let later = wb::merge_bg_restack_order(Some(fallback), 1500, wb::BG_RESTACK_FALLBACK_MS, false);
    assert_eq!(later.due, 1500 + wb::BG_RESTACK_FALLBACK_MS, "вторая просьба отодвинула срок");
    assert_eq!(later.requested, 1000, "точка отсчёта — первая просьба");
    let shorter = wb::merge_bg_restack_order(Some(later), 1600, 100, false);
    assert_eq!(shorter.due, later.due, "короткая просьба по сроку не подтягивает");

    let shown = wb::merge_bg_restack_order(Some(later), 1700, 0, true);
    assert_eq!(shown, wb::BgRestackOrder { due: 1700 + wb::BG_RESTACK_SHOWN_MS, requested: 1700, slack: 0 });
    assert!(shown.due < later.due, "первый кадр не ждёт запасного срока");
    assert!(wb::BG_RESTACK_SHOWN_MS < 500 && wb::BG_RESTACK_FALLBACK_MS > wb::BG_RESTACK_DELAY_MS);
}

/// When to set the backdrop — by output path: after showing where a change of
/// the desktop picture does not cover our surface (X11 window, GNOME/Cinnamon
/// extension, Cinnamon 6.8 with layer-shell, KDE, Treeland); BEFORE showing
/// where the picture is held by a separate background layer surface (wlroots,
/// COSMIC, unrecognised Wayland). Not setting it — nowhere: on Cinnamon with
/// layer-shell without a backdrop the lock screen showed a stale picture (16.11.2026).
#[test]
fn backdrop_timing_follows_the_output_path() {
    use rexon_core::wallpaper_engine::{self as wpe, BackdropTiming::*};
    for de in ["mate", "xfce", "cinnamon", "cinnamon-mint", "budgie", "gnome", "lxqt", "kde", "deepin", "generic"] {
        assert_eq!(wpe::backdrop_timing(de, "x11"), AfterShow, "{de} x11");
    }
    for de in ["gnome", "cinnamon", "cinnamon-mint"] {
        assert_eq!(
            wpe::backdrop_timing(de, "wayland"),
            AfterShow,
            "{de}: расширение оболочки или (Cinnamon 6.8) слой под перезапущенной службой фона"
        );
    }
    assert_eq!(wpe::backdrop_timing("kde", "wayland"), AfterShow);
    assert_eq!(wpe::backdrop_timing("deepin", "wayland"), AfterShow, "Treeland");
    for de in ["sway", "hyprland", "niri", "river", "labwc", "wayfire", "cosmic", "budgie", "lxqt", "generic"] {
        assert_eq!(wpe::backdrop_timing(de, "wayland"), BeforeShow, "{de}: своя поверхность фона");
    }
}

/// Backdrop frame cache: the same clip and the same screen — the same file (a
/// repeated apply does not call ffmpeg); another screen, an edited file or
/// another file — another frame. The frame is scaled to the largest output.
#[test]
fn video_still_cache_key_and_scaling() {
    use rexon_core::wallpaper_engine as wpe;
    let screen = Some((2560, 1440));
    let key = wpe::video_still_key("/home/u/волны.mp4", 4096, 100, screen);
    assert_eq!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, Some((1920, 1080))));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 101, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4097, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/лес.mp4", 4096, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, None));
    assert_eq!(
        wpe::video_still_path("/home/u/.cache/", &key),
        format!("/home/u/.cache/rexon/video-stills/{key}.jpg")
    );
    assert_eq!(wpe::still_screen_size(&[(1920, 1080), (2560, 1440), (1080, 1920)]), Some((2560, 1440)));
    assert_eq!(wpe::still_screen_size(&[]), None);
    let scaled = wpe::video_still_argv("/v.mp4", "/o.jpg", 3.0, screen).join(" ");
    assert!(scaled.contains("scale=2560:1440:force_original_aspect_ratio=increase"), "{scaled}");
    assert!(scaled.ends_with("/o.jpg") && scaled.contains("-frames:v 1"), "{scaled}");
    let native = wpe::video_still_argv("/v.mp4", "/o.jpg", 3.0, None).join(" ");
    assert!(!native.contains("scale="), "{native}");
    assert_eq!(wpe::backdrop_ready_log_line(420), "подложка готова через 420 мс после видео");
}

/// Frame cache limit: within it — nothing is touched; above the count or
/// bytes — the oldest go away, the frame being set — never; unfinished
/// `*.part.jpg` — always.
#[test]
fn video_stills_cache_is_bounded() {
    use rexon_core::wallpaper_engine as wpe;
    let f = |i: usize, mtime: i64, bytes: u64| (format!("/c/{i:02}.jpg"), mtime, bytes);
    let small: Vec<_> = (0..5).map(|i| f(i, i as i64, 1000)).collect();
    assert!(wpe::video_still_evictions(&small, "/c/04.jpg").is_empty());

    let n = wpe::VIDEO_STILLS_MAX_FILES + 3;
    let many: Vec<_> = (0..n).map(|i| f(i, 100 - i as i64, 1000)).collect();
    let keep = format!("/c/{:02}.jpg", n - 1); // the oldest — but it is being set
    let gone = wpe::video_still_evictions(&many, &keep);
    assert_eq!(gone.len(), 3, "{gone:?}");
    assert!(!gone.contains(&keep));
    assert_eq!(gone[0], format!("/c/{:02}.jpg", n - 2), "старейшие — первыми: {gone:?}");

    let big = wpe::VIDEO_STILLS_MAX_BYTES / 4 + 1;
    let heavy: Vec<_> = (0..5).map(|i| f(i, i as i64, big)).collect();
    let gone = wpe::video_still_evictions(&heavy, "/c/04.jpg");
    assert_eq!(gone, vec!["/c/00.jpg".to_string(), "/c/01.jpg".to_string()]);

    let mut part = small.clone();
    part.push(("/c/x.jpg.part.jpg".to_string(), 50, 10));
    assert_eq!(wpe::video_still_evictions(&part, "/c/04.jpg"), vec!["/c/x.jpg.part.jpg".to_string()]);
}

/// The «first frame on screen» measurement line: from «applying» and from the
/// host start; without the moment of applying — only from the start. Garbage
/// and a time from the future in the variable are not a measurement.
#[test]
fn first_frame_measure_line() {
    use rexon_core::wallpaper_engine as wpe;
    assert_eq!(wpe::since_apply_ms("1000", 2500), Some(1500));
    assert_eq!(wpe::since_apply_ms(" 1000\n", 1000), Some(0));
    assert_eq!(wpe::since_apply_ms("", 2500), None);
    assert_eq!(wpe::since_apply_ms("abc", 2500), None);
    assert_eq!(wpe::since_apply_ms("3000", 2500), None);
    assert_eq!(wpe::since_apply_ms("0", 2500), None);
    assert_eq!(
        wpe::first_frame_log_line("видео", Some(1500), 900),
        "первый кадр (видео) на экране через 1500 мс после «применяю» (хост — 900 мс от запуска)"
    );
    assert_eq!(wpe::first_frame_log_line("Flash", None, 700), "первый кадр (Flash) на экране через 700 мс от запуска хоста");
}
