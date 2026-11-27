//! Deepin (DDE): what Rexon can do in a Treeland session and what it refuses
//! outside of it. ALL of Rexon's knowledge about where Deepin keeps its
//! settings lives here; the theme, sound, font and wallpaper modules take
//! ready-made argv and texts from here.
//!
//! Rexon supports Deepin ONLY on Treeland, Deepin's Wayland compositor
//! (Wehrwolfmann's decision, 26.10.2026; it is installed by
//! [`crate::deepin_treeland`]). Rexon does not configure an X11 session with
//! deepin-kwin or any other Deepin session that is not on Treeland: there it
//! gives one honest sentence, "Rexon supports Deepin only on Treeland", and a
//! way to switch ([`not_treeland_message`]). A Treeland session is recognized
//! by its compositor — the process on the other end of the Wayland socket
//! ([`treeland_session_live`]) — and not by `XDG_SESSION_TYPE=wayland`.
//!
//! Checked against the linuxdeepin sources and the Arch packages (02.10.2026):
//!
//! * DConfig: keys that Deepin itself reads in any session (the sound theme,
//!   the current themes and fonts, the dde-shell dock) are written and read
//!   by the stock utility `dde-dconfig` (deepin-app-services); the daemon
//!   takes the user from the caller, root is not needed;
//! * wallpaper goes through the Control Center
//!   ([`control_center_wallpaper_argv`]): in Treeland, personal settings go
//!   only through it.
//!
//! Warning — security (openSUSE dropped Deepin in 2025 for bypassing the
//! review of D-Bus services and polkit): in Deepin, Rexon ONLY calls Deepin's
//! own user-facing interfaces. There are no services, polkit rules or system
//! files of our own here, and there must not be.
//!
//! Warning: `--auto-start=no` on bus service calls: these services are started
//! by D-Bus activation, and without this prohibition a call from ANOTHER
//! environment (where Deepin is merely installed) would bring up a Deepin
//! service in someone else's session.

use crate::i18n::tr;

/// DDE v23/v25 event sounds (session bus, dde-daemon `soundeffect1`).
pub const SOUND_EFFECT_DEST: &str = "org.deepin.dde.SoundEffect1";
pub const SOUND_EFFECT_PATH: &str = "/org/deepin/dde/SoundEffect1";

/// A DConfig key: application, resource (the schema name in `/usr/share/dsg/configs`), key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DConfigKey {
    pub appid: &'static str,
    pub resource: &'static str,
    pub key: &'static str,
}

const fn appearance_key(key: &'static str) -> DConfigKey {
    DConfigKey { appid: "org.deepin.dde.appearance", resource: "org.deepin.dde.appearance", key }
}

/// The `org.deepin.dde.appearance` schema (package deepin-appearance): the
/// current themes and fonts (read-only) and the sound theme.
pub const GTK_THEME: DConfigKey = appearance_key("Gtk_Theme");
pub const ICON_THEME: DConfigKey = appearance_key("Icon_Theme");
pub const CURSOR_THEME: DConfigKey = appearance_key("Cursor_Theme");
pub const CURSOR_SIZE: DConfigKey = appearance_key("cursorSize");
pub const FONT_STANDARD: DConfigKey = appearance_key("Font_Standard");
pub const FONT_MONOSPACE: DConfigKey = appearance_key("Font_Monospace");
pub const SOUND_THEME: DConfigKey = appearance_key("Sound_Theme");
/// The `org.deepin.XSettings` schema (package deepin-services): the XSettings
/// service hands it out to GTK programs.
pub const XS_SOUND_THEME: DConfigKey =
    DConfigKey { appid: "org.deepin.dde.daemon", resource: "org.deepin.XSettings", key: "sound-theme-name" };
/// Event sounds, the master switch (dde-daemon `soundeffect1`).
pub const EVENT_SOUNDS: DConfigKey = DConfigKey {
    appid: "org.deepin.dde.daemon",
    resource: "org.deepin.dde.daemon.soundeffect",
    key: "enabled",
};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
}

/// The master switch for event sounds: the `Enabled` property of the sound
/// service (its write handler also passes the value on to the theme player).
pub fn event_sounds_argv(enabled: bool) -> Vec<String> {
    s(&[
        "busctl", "--user", "--auto-start=no", "set-property", SOUND_EFFECT_DEST,
        SOUND_EFFECT_PATH, SOUND_EFFECT_DEST, "Enabled", "b",
        if enabled { "true" } else { "false" },
    ])
}

/// `dde-dconfig set`: writes a DConfig key on behalf of the caller.
pub fn dconfig_set_argv(k: DConfigKey, value: &str) -> Vec<String> {
    s(&["dde-dconfig", "set", "-a", k.appid, "-r", k.resource, "-k", k.key, "-v", value])
}

/// `dde-dconfig get`: the current value. The utility prints strings in double
/// quotes (`"deepin"`), numbers and booleans as they are.
pub fn dconfig_get_argv(k: DConfigKey) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", k.appid, "-r", k.resource, "-k", k.key])
}

// ── DConfig by keys known only at runtime (dock copy) ────────────────

/// The DConfig resource schema: `/usr/share/dsg/configs/<appid>/<resource>.json`.
pub fn dconfig_schema_path(appid: &str, resource: &str) -> String {
    format!("/usr/share/dsg/configs/{appid}/{resource}.json")
}

/// Whether a DConfig key name is fit for a command (names come from someone else's schema file).
pub fn dconfig_key_ok(key: &str) -> bool {
    !key.is_empty() && !key.starts_with('-') && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Schema keys that can be written (`permissions: readwrite`), in schema
/// order. If the schema cannot be read, the result is empty.
pub fn dconfig_schema_writable_keys(schema: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(schema) else {
        return Vec::new();
    };
    let Some(contents) = v.get("contents").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    contents
        .iter()
        .filter(|(k, meta)| {
            dconfig_key_ok(k) && meta.get("permissions").and_then(serde_json::Value::as_str) == Some("readwrite")
        })
        .map(|(k, _)| k.clone())
        .collect()
}

pub fn dconfig_get_key_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", appid, "-r", resource, "-k", key])
}

/// Whether the key holds its default value (answer `true` / `false`).
pub fn dconfig_is_default_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", appid, "-r", resource, "-k", key, "-m", "isDefaultValue"])
}

pub fn dconfig_set_key_argv(appid: &str, resource: &str, key: &str, value: &str) -> Vec<String> {
    s(&["dde-dconfig", "set", "-a", appid, "-r", resource, "-k", key, "-v", value])
}

/// Resets the key to its default value.
pub fn dconfig_reset_key_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "reset", "-a", appid, "-r", resource, "-k", key])
}

/// Converts a `dde-dconfig get` answer into what `dde-dconfig set -v`
/// accepts. The utility prints booleans and floats as they are, everything
/// else in double quotes, and lists and dictionaries as pretty-printed JSON
/// inside quotes; `set` parses a JSON list or dictionary and takes anything
/// else as a string.
pub fn dconfig_cli_value(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        let inner = &t[1..t.len() - 1];
        let body = inner.trim();
        if body.starts_with('[') || body.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                return v.to_string();
            }
        }
        return inner.to_string();
    }
    t.to_string()
}

/// The value from a `dde-dconfig get` or `busctl get-property` answer, without
/// the type signature (`i `, `s `, `d `, `b `) and quotes.
pub fn parse_value(raw: &str) -> String {
    let v = raw.trim();
    let v = match v.split_once(' ') {
        Some((sig, rest)) if matches!(sig, "s" | "i" | "u" | "d" | "b" | "y" | "n" | "q" | "x" | "t") => rest,
        _ => v,
    };
    v.trim().trim_matches('"').trim().to_string()
}

// ============================================================================
// The Treeland session and the refusal outside of it
// ============================================================================
//
// In a Treeland session, windows, the cursor and the wallpaper are drawn by
// the Treeland compositor, and it does NOT take its appearance from the
// appearance service: the service skips the global theme there ("in Treeland
// all personal settings go through the Control Center protocol"), the
// wallpaper `Set("background")` goes nowhere, and Treeland receives the icon
// and cursor themes, fonts, corners and transparency from the Control Center
// over its own Wayland protocol (`treeland_personalization_manager_v1`, in
// newer releases `treeland_appearance_manager_v1`, declared privileged:
// "for the Control Center only"). Treeland stores the values in the DConfig
// `org.deepin.dde.treeland` / `org.deepin.dde.treeland.user` of its own
// process, and in a DDM session that process runs as the service user
// `dde`, so they cannot be written on behalf of the person. Hence here: the
// wallpaper goes through the Control Center itself
// (`ShowPage("personalization/wallpaper?url=…")`), everything else gets an
// honest refusal ([`treeland_message`]).

/// The process name of the Treeland compositor (`/proc/<pid>/comm`).
pub const TREELAND_PROCESS: &str = "treeland";

/// The DDE Control Center (session bus).
pub const CONTROL_CENTER_DEST: &str = "org.deepin.dde.ControlCenter1";
pub const CONTROL_CENTER_PATH: &str = "/org/deepin/dde/ControlCenter1";

/// The session's Wayland socket: `WAYLAND_DISPLAY` is a path or a name in
/// `XDG_RUNTIME_DIR`. `None` means the session is not Wayland.
pub fn wayland_socket_path(wayland_display: &str, runtime_dir: &str) -> Option<String> {
    let name = wayland_display.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Some(name.to_string());
    }
    (!runtime_dir.is_empty()).then(|| format!("{}/{name}", runtime_dir.trim_end_matches('/')))
}

/// The compositor is Treeland: that is the name of the process on the other end of the Wayland socket.
pub fn is_treeland_process(comm: Option<&str>) -> bool {
    comm.map(str::trim) == Some(TREELAND_PROCESS)
}

/// The name of the process on the other end of the session's Wayland socket
/// (`SO_PEERCRED` → pid → `/proc/<pid>/comm`; `comm` can be read for another
/// user too — in a DDM session Treeland runs as `dde`). `None` means there is
/// no socket or the kernel did not answer.
pub fn wayland_compositor_live() -> Option<String> {
    use std::os::fd::AsRawFd;
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    let path = wayland_socket_path(&env("WAYLAND_DISPLAY"), &env("XDG_RUNTIME_DIR"))?;
    let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the buffer is a live `ucred` of the right size, and the length is passed correctly.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 || cred.pid <= 0 {
        return None;
    }
    std::fs::read_to_string(format!("/proc/{}/comm", cred.pid)).ok().map(|c| c.trim().to_string())
}

/// Whether a Treeland session is running right now, judged by the compositor, not by the session type.
pub fn treeland_session_live() -> bool {
    is_treeland_process(wayland_compositor_live().as_deref())
}

/// The refusal "in Treeland this is set only by the Control Center".
pub fn treeland_message() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("In the Deepin Wayland session (Treeland) this is set only by Deepin's Control Center — Rexon can't change it there yet: use Control Center → Personalization.")
}

/// The refusal in any Deepin session not on Treeland (X11 with deepin-kwin and
/// others): one sentence and a way to switch.
pub fn not_treeland_message() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    // Login into Treeland is only possible through DDM (treeland#1656), see `deepin_treeland`.
    tr("Rexon supports Deepin only on Treeland. To switch, open Rexon → Desktops → Deepin → “Switch to Deepin on Treeland”, then make DDM the login screen and choose Treeland there.")
}

/// Why Rexon does not change this in Deepin: in Treeland it is set only by the
/// Control Center; outside Treeland, see [`not_treeland_message`].
pub fn refusal(treeland: bool) -> String {
    if treeland {
        treeland_message()
    } else {
        not_treeland_message()
    }
}

/// [`refusal`] for the session that is running now.
pub fn refusal_live() -> String {
    refusal(treeland_session_live())
}

/// Wallpaper through the Control Center: the command for the "Wallpaper" page
/// with the picture's address. With a command, the Control Center does not
/// open a window: it sets the wallpaper through its own Treeland path (on the
/// screen currently selected in it) and answers once it has found the page.
/// `lock` means the lock screen instead of the desktop.
///
/// Warning: no `--auto-start=no` here: the Control Center is started by D-Bus
/// activation on the first call, and this is called only in a Treeland
/// session. The address is percent-encoded: the Control Center splits the
/// command at `&` and `=`.
pub fn control_center_wallpaper_argv(path: &str, lock: bool) -> Vec<String> {
    let url = crate::appearance_installer::path_as_uri(path);
    let kind = if lock { "lock" } else { "desktop" };
    let page = format!("personalization/wallpaper?url={url}&type={kind}");
    s(&[
        "busctl", "--user", "--timeout=60", "call", CONTROL_CENTER_DEST, CONTROL_CENTER_PATH,
        CONTROL_CENTER_DEST, "ShowPage", "s", &page,
    ])
}

/// The result of handing the wallpaper to the Control Center: it cannot be
/// read back from Treeland, so we report what was done, without "✅ set".
pub fn wallpaper_handed_message() -> String {
    tr("Wallpaper handed to Deepin's Control Center — it sets it on the screen selected there.")
}

/// The Control Center did not accept the wallpaper.
pub fn control_center_failed_message() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Deepin's Control Center didn't take the wallpaper — set it in Control Center → Personalization → Wallpaper.")
}

// ============================================================================
// The "Deepin windows" section of the "Settings" tab
// ============================================================================

/// A snapshot of the "Deepin windows" section: Treeland takes window corners,
/// transparency and accent only from the Control Center, so the section is an
/// explanation of where to change them (in Treeland) or that Rexon supports
/// Deepin only on Treeland.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LookState {
    /// The section is shown only in a Deepin session.
    pub available: bool,
    pub note: String,
}

/// A snapshot of the section. `de` is the environment as the core names it;
/// `treeland` is whether a Treeland session is running ([`treeland_session_live`]).
pub fn read_state(de: &str, treeland: bool) -> LookState {
    LookState { available: de == "deepin", note: refusal(treeland) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bus_call_refuses_autostart() {
        let argv = event_sounds_argv(true);
        assert_eq!(argv[0], "busctl");
        assert_eq!(argv[1], "--user", "только шина сеанса: {argv:?}");
        assert_eq!(argv[2], "--auto-start=no", "{argv:?}");
    }

    #[test]
    fn values_parse_from_both_tools() {
        assert_eq!(parse_value("\"deepin\"\n"), "deepin");
        assert_eq!(parse_value("s \"Noto Sans\""), "Noto Sans");
        assert_eq!(parse_value("i 12\n"), "12");
    }

    /// A `dde-dconfig get` answer → the value for `set -v`: quotes are removed,
    /// pretty-printed JSON of lists and dictionaries is compacted, booleans stay
    /// as they are.
    #[test]
    fn dconfig_cli_values_round_trip() {
        assert_eq!(dconfig_cli_value("\"bottom\"\n"), "bottom");
        assert_eq!(dconfig_cli_value("\"48\"\n"), "48");
        assert_eq!(dconfig_cli_value("true\n"), "true");
        assert_eq!(dconfig_cli_value("\"[\n    \"a\",\n    \"b c\"\n]\n\"\n"), "[\"a\",\"b c\"]");
        assert_eq!(dconfig_cli_value("\"{\n}\n\""), "{}");
        assert_eq!(dconfig_cli_value("\"\""), "");
        let schema = r#"{"contents": {"b": {"permissions": "readwrite"}, "a": {"permissions": "readonly"}, "-x": {"permissions": "readwrite"}, "c": {"permissions": "readwrite"}}}"#;
        assert_eq!(dconfig_schema_writable_keys(schema), ["b", "c"]);
        assert!(dconfig_schema_writable_keys("not json").is_empty());
    }

    /// A Treeland session is recognized by the compositor process on the other
    /// end of the Wayland socket, not by the session type.
    #[test]
    fn treeland_is_recognised_by_its_compositor() {
        assert_eq!(wayland_socket_path("wayland-0", "/run/user/1000/").as_deref(), Some("/run/user/1000/wayland-0"));
        assert_eq!(wayland_socket_path("/tmp/w", "").as_deref(), Some("/tmp/w"));
        assert_eq!(wayland_socket_path("", "/run/user/1000"), None, "сеанс X11");
        assert_eq!(wayland_socket_path("wayland-0", ""), None);
        assert!(is_treeland_process(Some("treeland\n")));
        for other in [Some("kwin_wayland"), Some("deepin-kwin_wayland"), Some("treeland-sd"), None] {
            assert!(!is_treeland_process(other), "{other:?}");
        }
    }

    /// The refusal: in Treeland, "only the Control Center"; in any other Deepin
    /// session, "Rexon supports Deepin only on Treeland" with a way to switch.
    #[test]
    fn refusal_depends_on_the_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(refusal(true), treeland_message());
        let other = refusal(false);
        assert!(other.starts_with("Rexon supports Deepin only on Treeland"), "{other}");
        assert!(other.contains("Switch to Deepin on Treeland"), "{other}");
    }

    /// Rexon does not write Deepin's GTK, icon and cursor themes in any session:
    /// the plan is a single stop before writing (the reason is [`refusal`] for
    /// the session), and GNOME keys are not touched.
    #[test]
    fn deepin_theme_plans_stop_before_writing() {
        use crate::themes::{self, ApplyStep};
        let gtk = themes::apply_gtk_plan_xdg("Materia", "deepin", "/h", None);
        assert_eq!(gtk.steps, vec![ApplyStep::DdeTreelandGate]);
        let icons = themes::apply_icon_plan_xdg("Papirus", "deepin", "/h", None, true).unwrap();
        assert_eq!(icons.steps, vec![ApplyStep::DdeTreelandGate]);
        let cur = themes::apply_cursor_plan_sized("Bibata", Some(32), "deepin", "/h", None, true).unwrap();
        assert_eq!(cur.steps[0], ApplyStep::DdeTreelandGate);
        assert!(!format!("{:?}", cur.steps).contains("org.gnome.desktop.interface"));
        let q = themes::cursor_queries("deepin", "/h", None, false);
        assert!(matches!(&q[0], themes::CursorQuery::Cmd(c) if c.argv == dconfig_get_argv(CURSOR_THEME)));
    }

    /// Deepin sounds go into its DConfig keys (Deepin itself reads them in any
    /// session); fonts are read-only.
    #[test]
    fn deepin_sounds_use_native_keys_and_fonts_are_read_only() {
        use crate::qt_settings as qs;
        let plan = qs::sound_plan("deepin", qs::SoundSetting::Theme, "freedesktop", "", "", true);
        assert_eq!(plan.write[0], dconfig_set_argv(SOUND_THEME, "freedesktop"));
        assert!(plan.write.contains(&dconfig_set_argv(XS_SOUND_THEME, "freedesktop")));
        assert!(plan.write.iter().any(|a| a.contains(&"org.gnome.desktop.sound".to_string())));
        assert_eq!(plan.verify, Some(dconfig_get_argv(SOUND_THEME)));
        let ev = qs::sound_plan("deepin", qs::SoundSetting::EventSounds, "false", "", "", false);
        assert_eq!(ev.write[0], event_sounds_argv(false));
        assert_eq!(ev.verify, Some(dconfig_get_argv(EVENT_SOUNDS)));
        for target in ["system", "mono"] {
            let fp = qs::font_plan("deepin", target);
            assert_eq!(qs::font_store_write_argv(&fp.primary, "Noto Sans", 11, ""), None, "{target}");
        }
        assert!(!qs::font_target_available("deepin", "title"), "шрифта заголовков у Deepin нет");
    }

    /// Wallpaper: in Treeland it is accepted by the Control Center via its page
    /// command (the message honestly says "handed over"); outside Treeland it is
    /// refused, without third-party renderers.
    #[test]
    fn deepin_wallpaper_goes_to_control_center_only_in_treeland() {
        use crate::wallpaper_engine::{static_plan, TREELAND_SESSION};
        let steps = static_plan("deepin", TREELAND_SESSION, "/h/a&b=c.png", &[], "");
        assert_eq!(steps[0].argv, control_center_wallpaper_argv("/h/a&b=c.png", false));
        let page = steps[0].argv.last().unwrap();
        assert_eq!(page, "personalization/wallpaper?url=file:///h/a%26b%3Dc.png&type=desktop");
        assert!(!steps[0].argv.contains(&"--auto-start=no".to_string()), "Центр управления заводится по вызову");
        assert!(!steps.iter().any(|s| s.argv.first().map(String::as_str) == Some("swaybg")));
        for session in ["x11", "wayland", ""] {
            let other = static_plan("deepin", session, "/h/a.png", &["feh".into(), "swaybg".into()], "");
            assert!(other.iter().all(|s| s.argv.is_empty()), "{session}: {other:?}");
        }
    }

    /// The "Deepin windows" section: only an explanation; hidden outside Deepin.
    #[test]
    fn look_section_explains_instead_of_writing() {
        assert_eq!(read_state("deepin", true), LookState { available: true, note: treeland_message() });
        assert_eq!(read_state("deepin", false).note, not_treeland_message());
        assert!(!read_state("kde", true).available);
    }

    /// Recognition: v23/v25 (`DDE`), deepin 20 (`Deepin`), a Treeland Wayland
    /// session and the session names are all Deepin; it is present in every place
    /// in Rexon that lists environments.
    #[test]
    fn deepin_is_recognised_everywhere() {
        use crate::system::detect_current_de;
        for (xdg, session) in [("DDE", "deepin"), ("Deepin", ""), ("Treeland", "treeland"), ("", "deepin"), ("", "treeland-user")] {
            assert_eq!(detect_current_de(xdg, session, false).as_deref(), Some("deepin"), "{xdg:?} {session:?}");
        }
        assert_eq!(crate::de_customization::de_from_id("deepin"), Some(crate::de_customization::De::Deepin));
        assert!(crate::system::DE_META.iter().any(|m| m.id == "deepin"));
        assert!(crate::system::DE_DATABASE.iter().any(|(id, _)| *id == "deepin"));
        assert_eq!(crate::system::de_installed_binaries("deepin"), &["dde-session"]);
        assert_eq!(crate::appdefaults::resolve_mimeapps_filename("deepin"), "dde-mimeapps.list");
        assert!(!crate::themes::is_generic_wm("deepin"));
    }

    /// The Deepin terminal has its own schema (`app-id` without `.desktop`).
    #[test]
    fn deepin_terminal_goes_to_its_schema() {
        use crate::defaults as d;
        assert_eq!(d::terminal_home("DDE"), d::TerminalHome::Deepin);
        assert_eq!(d::terminal_exec_schema("DDE"), Some(d::DEEPIN_TERMINAL_SCHEMA));
        let steps = d::terminal_home_steps(d::TerminalHome::Deepin, "deepin-terminal.desktop", "/h/.config");
        let argvs: Vec<Vec<String>> = steps
            .into_iter()
            .filter_map(|s| match s {
                d::TermStep::Run { argv } => Some(argv),
                _ => None,
            })
            .collect();
        assert_eq!(argvs[0], s(&["gsettings", "set", d::DEEPIN_TERMINAL_SCHEMA, "app-id", "deepin-terminal"]));
        assert!(argvs.contains(&s(&["gsettings", "set", d::DEEPIN_TERMINAL_SCHEMA, "exec", "deepin-terminal"])));
        assert_eq!(
            d::get_current_terminal(None, "DDE", None, Some("'deepin-terminal'\n")).as_deref(),
            Some("deepin-terminal.desktop")
        );
    }
}
