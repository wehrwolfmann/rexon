//! SDDM login screen: theme background and carrying over the desktop look
//! (parity with the KDE module «Login Screen (SDDM)», sddm-kcm 6.8). Pure core
//! decisions (`system_plan::plan_sddm_look`, `sddm_theme_background`,
//! `sddm_background_user_text`, `desktop_greeter_look`) — on mocked files; the
//! expected edit texts are the reference `golden/sddm_greeter_cases.json`
//! (recapture: `REXON_GOLDEN_WRITE=1 cargo test-core --test sddm_greeter`,
//! then review the diff by eye).

use std::collections::BTreeMap;

use rexon_core::system_plan::{self as plan, SessionSwitch, SddmLook, SddmNumlock};
use serde_json::{json, Value};

const GOLDEN: &str = "tests/golden/sddm_greeter_cases.json";

/// Mock system: path → text.
fn fs(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

fn plan_on(files: &BTreeMap<String, String>, look: Option<&SddmLook>) -> SessionSwitch {
    let read = |p: &str| files.get(p).cloned();
    let list = |d: &str| -> Vec<String> {
        let prefix = format!("{d}/");
        files.keys().filter_map(|p| p.strip_prefix(&prefix)).filter(|r| !r.contains('/')).map(str::to_string).collect()
    };
    plan::plan_sddm_look(look, &read, &list)
}

fn apply(files: &BTreeMap<String, String>, sw: &SessionSwitch) -> BTreeMap<String, String> {
    let mut out = files.clone();
    if let SessionSwitch::Edits(e) = sw {
        for e in e {
            out.insert(e.path.clone(), e.text.clone());
        }
    }
    out
}

fn switch_json(sw: &SessionSwitch) -> Value {
    match sw {
        SessionSwitch::Nothing => json!("nothing"),
        SessionSwitch::Elsewhere => json!("elsewhere"),
        SessionSwitch::Edits(e) => {
            let m: BTreeMap<String, String> = e.iter().map(|e| (e.path.clone(), e.text.clone())).collect();
            json!(m)
        }
    }
}

/// The key value as SDDM will read it.
fn effective(files: &BTreeMap<String, String>, group: &str, key: &str) -> Option<String> {
    let mut order: Vec<&String> = Vec::new();
    for dir in [plan::SDDM_DISTRO_CONF_D, plan::SDDM_CONF_D] {
        let mut names: Vec<&String> =
            files.keys().filter(|p| p.starts_with(&format!("{dir}/")) && p.ends_with(".conf")).collect();
        names.sort();
        order.extend(names);
    }
    let main = "/etc/sddm.conf".to_string();
    let texts: Vec<String> = order.into_iter().chain([&main]).filter_map(|p| files.get(p).cloned()).collect();
    rexon_core::login_rescue::ini_value(&texts, group, key)
}

/// A machine like CachyOS: package values in `/usr/lib`, KDE settings in
/// `kde_settings.conf`, autologin in `/etc/sddm.conf`.
fn cachyos() -> BTreeMap<String, String> {
    fs(&[
        (
            "/usr/lib/sddm/sddm.conf.d/default.conf",
            "[General]\nNumlock=none\n\n[Theme]\nCurrent=\nCursorTheme=\nFont=\n\n[X11]\nServerArguments=-nolisten tcp\n",
        ),
        (
            "/etc/sddm.conf.d/kde_settings.conf",
            "[General]\nGreeterEnvironment=QML_DISABLE_DISTANCEFIELD=1\n\n[Theme]\nCurrent=breeze\nCursorSize=32\n\
             CursorTheme=breeze_cursors\nFont=Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n",
        ),
        ("/etc/sddm.conf", "[Autologin]\nSession=plasma\n"),
    ])
}

fn plasma_look() -> SddmLook {
    SddmLook {
        cursor_theme: Some("Bibata-Modern-Ice".into()),
        cursor_size: Some(24),
        font: Some("Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1".into()),
        numlock: Some(SddmNumlock::On),
        dpi: Some(120),
    }
}

/// All cases go into one reference.
fn cases() -> Value {
    let mut out = serde_json::Map::new();
    let base = cachyos();
    let sync = plan_on(&base, Some(&plasma_look()));
    out.insert("cachyos_sync".into(), switch_json(&sync));
    let synced = apply(&base, &sync);
    let reset = plan_on(&synced, None);
    out.insert("cachyos_reset_after_sync".into(), switch_json(&reset));
    // The key is set in `/etc/sddm.conf` (SDDM reads it last): it is edited
    // there, not duplicated in `kde_settings.conf`.
    let mut over = cachyos();
    over.insert("/etc/sddm.conf".into(), "[Autologin]\nSession=plasma\n\n[Theme]\nCursorTheme=Adwaita\n".into());
    out.insert("override_in_sddm_conf".into(), switch_json(&plan_on(&over, Some(&plasma_look()))));
    // Clean machine: no own settings — the KDE file is created.
    let bare = fs(&[("/usr/lib/sddm/sddm.conf.d/default.conf", "[X11]\nServerArguments=-nolisten tcp\n")]);
    let minimal = SddmLook { cursor_theme: Some("Adwaita".into()), ..SddmLook::default() };
    out.insert("bare_machine".into(), switch_json(&plan_on(&bare, Some(&minimal))));
    // Theme background: the `.user` text.
    out.insert(
        "background_user_new".into(),
        json!(plan::sddm_background_user_text(None, Some("rexon-background.jpg"))),
    );
    out.insert(
        "background_user_keep_clock".into(),
        json!(plan::sddm_background_user_text(Some("[General]\nshowClock=false\ntype=color\n"), Some("rexon-background.png"))),
    );
    out.insert(
        "background_user_reset".into(),
        json!(plan::sddm_background_user_text(
            Some("[General]\nshowClock=false\nbackground=rexon-background.png\ntype=image\n"),
            None
        )),
    );
    // «Show clock»: the key is written explicitly (even for «yes»), the background is not touched.
    out.insert(
        "clock_user_off".into(),
        json!(plan::sddm_clock_user_text(Some("[General]\nbackground=rexon-background.png\ntype=image\n"), false)),
    );
    out.insert("clock_user_new_on".into(), json!(plan::sddm_clock_user_text(None, true)));
    Value::Object(out)
}

#[test]
fn login_screen_edits_golden() {
    let got = cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(include_str!("golden/sddm_greeter_cases.json")).unwrap();
    assert_eq!(got, want, "эталон {GOLDEN} разошёлся");
}

/// After the carry-over SDDM reads exactly the desktop values; the package's
/// `-nolisten tcp` is not lost (the KDE module loses it); after «Reset» —
/// the package values and the former X server arguments again.
#[test]
fn transfer_and_reset_by_sddm_read_result() {
    let base = cachyos();
    let synced = apply(&base, &plan_on(&base, Some(&plasma_look())));
    assert_eq!(effective(&synced, "Theme", "CursorTheme").as_deref(), Some("Bibata-Modern-Ice"));
    assert_eq!(effective(&synced, "Theme", "CursorSize").as_deref(), Some("24"));
    assert_eq!(effective(&synced, "Theme", "Font").as_deref(), Some("Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
    assert_eq!(effective(&synced, "General", "Numlock").as_deref(), Some("on"));
    assert_eq!(effective(&synced, "X11", "ServerArguments").as_deref(), Some("-nolisten tcp -dpi 120"));
    assert_eq!(effective(&synced, "Theme", "Current").as_deref(), Some("breeze"), "тема не тронута");
    assert_eq!(synced["/etc/sddm.conf"], base["/etc/sddm.conf"], "автовход не тронут");
    // A repeat — nothing to change.
    assert_eq!(plan_on(&synced, Some(&plasma_look())), SessionSwitch::Nothing);
    let reset = apply(&synced, &plan_on(&synced, None));
    assert_eq!(effective(&reset, "X11", "ServerArguments").as_deref(), Some("-nolisten tcp"));
    for (g, k) in [("Theme", "CursorTheme"), ("Theme", "Font"), ("General", "Numlock")] {
        assert_eq!(effective(&reset, g, k), effective(&fs(&[("/usr/lib/sddm/sddm.conf.d/default.conf", &base["/usr/lib/sddm/sddm.conf.d/default.conf"])]), g, k), "{g}/{k}");
    }
    assert!(!reset["/etc/sddm.conf.d/kde_settings.conf"].contains("ServerArguments"));
    assert!(reset["/etc/sddm.conf.d/kde_settings.conf"].contains("Current=breeze"));
    // A repeated reset — nothing to change.
    assert_eq!(plan_on(&reset, None), SessionSwitch::Nothing);
}

/// Font, NumLock and DPI are not set — their keys are not touched (as in the
/// KDE module): only the cursor changes.
#[test]
fn cursor_untouched_when_dpi_unset() {
    let base = cachyos();
    let look = SddmLook { dpi: None, numlock: None, font: None, ..plasma_look() };
    let SessionSwitch::Edits(e) = plan_on(&base, Some(&look)) else { panic!("ждали правки") };
    assert_eq!(e.len(), 1);
    assert!(!e[0].text.contains("ServerArguments") && !e[0].text.contains("Numlock"));
}

#[test]
fn x_server_arguments() {
    assert_eq!(plan::server_args_with_dpi("-nolisten tcp", Some(96)), "-nolisten tcp -dpi 96");
    assert_eq!(plan::server_args_with_dpi("-dpi 120 -nolisten tcp", Some(96)), "-nolisten tcp -dpi 96");
    assert_eq!(plan::server_args_with_dpi("-nolisten tcp -dpi 120", None), "-nolisten tcp");
    assert_eq!(plan::server_args_with_dpi("", None), "");
}

#[test]
fn theme_background_like_kde_module() {
    let dir = "/usr/share/sddm/themes/breeze";
    let files = fs(&[
        ("/usr/share/sddm/themes/breeze/metadata.desktop", "[SddmGreeterTheme]\nName=Breeze\nConfigFile=theme.conf\n"),
        (
            "/usr/share/sddm/themes/breeze/theme.conf",
            "[General]\ntype=image\nbackground=/usr/share/wallpapers/Next/contents/images/5120x2880.png\n",
        ),
    ]);
    let read = |p: &str| files.get(p).cloned();
    let bg = plan::sddm_theme_background(dir, &read);
    assert!(bg.supported && !bg.custom && !bg.ours);
    assert_eq!(bg.image, "/usr/share/wallpapers/Next/contents/images/5120x2880.png");
    assert_eq!(bg.config, "/usr/share/sddm/themes/breeze/theme.conf");
    // Own background on top — a relative name in the theme directory.
    let mut with_user = files.clone();
    with_user.insert(
        "/usr/share/sddm/themes/breeze/theme.conf.user".into(),
        "[General]\nbackground=rexon-background.jpg\ntype=image\n".into(),
    );
    let read = |p: &str| with_user.get(p).cloned();
    let bg = plan::sddm_theme_background(dir, &read);
    assert!(bg.custom && bg.ours);
    assert_eq!(bg.image, "/usr/share/sddm/themes/breeze/rexon-background.jpg");
    // A theme without a settings file and a path instead of a name — the background does not change.
    for meta in ["[SddmGreeterTheme]\nName=X\n", "[SddmGreeterTheme]\nConfigFile=../../../etc/passwd\n"] {
        let f = fs(&[("/t/metadata.desktop", meta), ("/etc/passwd", "root:x:0:0")]);
        let read = |p: &str| f.get(p).cloned();
        assert!(!plan::sddm_theme_background("/t", &read).supported, "{meta}");
    }
}

#[test]
fn image_by_signature() {
    assert_eq!(plan::login_image_ext(b"\x89PNG\r\n\x1a\n...."), Some("png"));
    assert_eq!(plan::login_image_ext(b"\xff\xd8\xff\xe0JFIF"), Some("jpg"));
    for bad in [&b"RIFF\0\0\0\0WEBP"[..], b"GIF89a", b"<svg", b"#!/bin/sh", b""] {
        assert_eq!(plan::login_image_ext(bad), None, "{bad:?}");
    }
    assert!(plan::sddm_background_is_ours("rexon-background.png"));
    assert!(!plan::sddm_background_is_ours("rexon-background.png.bak"));
    assert!(!plan::sddm_background_is_ours("background.jpg"));
}

/// Plasma: everything as in the KDE module (`kcminputrc`, `kdeglobals`,
/// `kcmfonts`, `.config` files); outside Plasma — the desktop cursor and font,
/// from files only Qt colours and fontconfig, the rest honestly «not carried over».
#[test]
fn what_is_taken_from_desktop() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let files = fs(&[
        ("/h/.config/kcminputrc", "[Keyboard]\nNumLock=1\n\n[Mouse]\ncursorTheme=Bibata-Modern-Ice\ncursorSize=32\n"),
        ("/h/.config/kdeglobals", "[General]\nfont=Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n"),
        ("/h/.config/kcmfonts", "[General]\nforceFontDPI=0\n"),
        ("/h/.config/plasmarc", "[Theme]\nname=default\n"),
        ("/h/.config/fontconfig/fonts.conf", "<fontconfig/>\n"),
    ]);
    let read = |p: &str| files.get(p).cloned();
    let exists = |p: &str| files.contains_key(p);
    let l = plan::desktop_greeter_look(true, "/h/.config", &read, &exists, &plan::OtherDesktopLook::default());
    assert_eq!(l.look.cursor_theme.as_deref(), Some("Bibata-Modern-Ice"));
    assert_eq!(l.look.cursor_size, Some(32));
    assert_eq!(l.look.numlock, Some(SddmNumlock::Off));
    assert_eq!(l.look.dpi, None, "forceFontDPI=0 — масштаб не задан, -dpi 0 не уходит");
    let names: Vec<&str> = l.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["kdeglobals", "plasmarc", "kcminputrc", "fontconfig/fonts.conf"]);
    assert!(l.missing.contains(&"Scaling DPI".to_string()) && l.missing.contains(&"Screen configuration (Wayland only)".to_string()));

    let other = plan::OtherDesktopLook {
        cursor_theme: Some("Adwaita".into()),
        cursor_size: Some(24),
        font: Some("Cantarell 11".into()),
    };
    let l = plan::desktop_greeter_look(false, "/h/.config", &read, &exists, &other);
    assert_eq!(l.look.font.as_deref(), Some("Cantarell,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
    assert_eq!(l.look.numlock, None);
    let names: Vec<&str> = l.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["kdeglobals", "fontconfig/fonts.conf"], "plasmarc и kcminputrc вне Plasma не берутся");
    assert!(l.missing.contains(&"Plasma theme".to_string()) && l.missing.contains(&"NumLock preference".to_string()));
    let view = plan::sddm_greeter_view(Some("breeze"), Some("Breeze"), &plan::SddmBackground::default(), &l, false, "");
    assert!(view.visible && view.can_change);
    assert_eq!(view.theme, "Theme: Breeze");
    assert!(view.look.confirm.contains("• Font") && view.look.confirm.contains("• Plasma theme"));
    assert!(!view.background.supported && !view.background.note.is_empty());
    // Built-in theme (nothing set) — no section.
    assert!(!plan::sddm_greeter_view(None, None, &plan::SddmBackground::default(), &l, false, "").visible);
}

/// «Show clock» — as in the kcm (`ThemesModel::add`): `showClock` from `.user`
/// beats the theme file, absent everywhere — yes; values as in KConfig.
/// Theme Qt version — `[SddmGreeterTheme] QtVersion`, absent — 5.
#[test]
fn clock_and_theme_qt_version() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let dir = "/usr/share/sddm/themes/breeze";
    let meta = "[SddmGreeterTheme]\nName=Breeze\nConfigFile=theme.conf\nQtVersion=6\n";
    let with = |base: &str, user: Option<&str>| {
        let mut f = fs(&[("/usr/share/sddm/themes/breeze/metadata.desktop", meta), ("/usr/share/sddm/themes/breeze/theme.conf", base)]);
        if let Some(u) = user {
            f.insert("/usr/share/sddm/themes/breeze/theme.conf.user".into(), u.into());
        }
        let read = |p: &str| f.get(p).cloned();
        plan::sddm_theme_background(dir, &read)
    };
    let bg = with("[General]\ntype=image\n", None);
    assert!(bg.clock, "нет ключа — часы показываются");
    assert_eq!(bg.qt, 6);
    assert!(!with("[General]\nshowClock=false\n", None).clock);
    assert!(with("[General]\nshowClock=false\n", Some("[General]\nshowClock=true\n")).clock, ".user сильнее темы");
    assert!(!with("[General]\n", Some("[General]\nshowClock=off\n")).clock);
    assert_eq!(plan::kconfig_bool(" Yes "), Some(true));
    assert_eq!(plan::kconfig_bool("0"), Some(false));
    assert_eq!(plan::kconfig_bool("maybe"), None);
    assert_eq!(plan::sddm_theme_qt_version("[SddmGreeterTheme]\nName=X\n"), 5);
    assert_eq!(plan::sddm_theme_qt_version("[SddmGreeterTheme]\nQtVersion=abc\n"), 5);
    // Writing the same again — nothing to change.
    assert_eq!(plan::sddm_clock_user_text(Some("[General]\nshowClock=true\n"), true), None);
    // Block view: the checkbox is on a theme with a settings file.
    let l = plan::DesktopLook::default();
    let v = plan::sddm_greeter_view(Some("breeze"), None, &bg, &l, true, "");
    assert!(v.clock.supported && v.clock.checked && !v.clock.label.is_empty());
    let none = plan::sddm_greeter_view(Some("x"), None, &plan::SddmBackground::default(), &l, true, "");
    assert!(!none.clock.supported);
}

/// WebP — by signature and only with the WebP module in the greeter's Qt; the
/// refusal before the password names what is missing; the picker filter
/// includes `*.webp`.
#[test]
fn webp_by_qt_module() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let webp = b"RIFF\x10\0\0\0WEBPVP8 ";
    assert!(plan::is_webp(webp));
    assert_eq!(plan::login_image_ext_with(webp, true), Some("webp"));
    assert_eq!(plan::login_image_ext_with(webp, false), None);
    assert_eq!(plan::login_image_ext_with(b"\x89PNG\r\n\x1a\n....", false), Some("png"));
    assert_eq!(plan::login_image_ext_with(b"RIFF\0\0\0\0WAVEfmt ", true), None, "RIFF, но не WebP");
    assert!(plan::login_image_refusal(webp, false, 5).contains("qt5-imageformats"));
    assert!(plan::login_image_refusal(b"GIF89a", true, 6).contains("WebP"));
    assert!(plan::login_image_filter(true).contains("*.webp"));
    assert!(!plan::login_image_filter(false).contains("webp"));
    assert!(plan::sddm_background_is_ours("rexon-background.webp"));
    // Module layouts: Qt 6 and Qt 5 are looked up in their own directories.
    let q6 = plan::qt_webp_plugin_paths(6);
    assert!(q6.contains(&"/usr/lib/qt6/plugins/imageformats/libqwebp.so".to_string()), "{q6:?}");
    let q5 = plan::qt_webp_plugin_paths(5);
    assert!(q5.contains(&"/usr/lib/qt/plugins/imageformats/libqwebp.so".to_string()), "{q5:?}");
    assert!(plan::qt_has_webp(6, &|p| p == "/usr/lib/qt6/plugins/imageformats/libqwebp.so"));
    assert!(!plan::qt_has_webp(5, &|p| p == "/usr/lib/qt6/plugins/imageformats/libqwebp.so"));
    // Block view: filter by the background's `webp`.
    let bg = plan::SddmBackground { supported: true, webp: true, ..Default::default() };
    let v = plan::sddm_greeter_view(Some("breeze"), None, &bg, &plan::DesktopLook::default(), true, "");
    assert!(v.background.filter.contains("*.webp"));
    // Client refusal before the password: WebP without the module — no request to the helper.
    let mut asked = false;
    let r = plan::sddm_background_with("breeze", Some("/x.webp"), false, 6, &|_| Some(webp.to_vec()), &mut |_, _| {
        asked = true;
        None
    });
    assert!(r.unwrap_err().contains("qt6-imageformats") && !asked);
}

/// Preview — the greeter matching the theme's Qt version, as in the SDDM 0.21
/// daemon: `sddm-greeter` for Qt 5, `sddm-greeter-qt6` for `QtVersion=6`;
/// flags — `--test-mode --theme <directory>`. No theme or greeter — a refusal in words.
#[test]
fn sddm_theme_preview() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let files = fs(&[
        ("/usr/share/sddm/themes/breeze/metadata.desktop", "[SddmGreeterTheme]\nName=Breeze\nQtVersion=6\n"),
        ("/usr/share/sddm/themes/old/metadata.desktop", "[SddmGreeterTheme]\nName=Old\n"),
    ]);
    let read = |p: &str| files.get(p).cloned();
    let both = |p: &str| p == "/usr/bin/sddm-greeter" || p == "/usr/bin/sddm-greeter-qt6";
    assert_eq!(
        plan::sddm_preview_argv("breeze", &read, &both).unwrap(),
        ["/usr/bin/sddm-greeter-qt6", "--test-mode", "--theme", "/usr/share/sddm/themes/breeze"]
    );
    assert_eq!(plan::sddm_preview_argv("old", &read, &both).unwrap()[0], "/usr/bin/sddm-greeter");
    let only6 = |p: &str| p == "/usr/bin/sddm-greeter-qt6";
    assert!(plan::sddm_preview_argv("old", &read, &only6).unwrap_err().contains("/usr/bin/sddm-greeter"));
    for bad in ["", "..", "../etc", "-x", "nope"] {
        assert!(plan::sddm_preview_argv(bad, &read, &both).is_err(), "{bad}");
    }
    // An SDDM theme tile in the grid gets the preview badge, a splash screen gets its own name.
    use rexon_core::appearance_page::local_splash_demo;
    assert_eq!(local_splash_demo("local:sddm:breeze").as_deref(), Some("sddm:breeze"));
    assert_eq!(local_splash_demo("local:sddm:-x"), None);
    assert_eq!(local_splash_demo("local:splash:Breeze").as_deref(), Some("Breeze"));
    let v = plan::greeter_preview_view(false, None);
    assert!(v.available && v.note.is_empty() && !v.close.is_empty());
    assert!(plan::greeter_preview_view(true, None).tooltip.contains("mypassword"));
    let off = plan::greeter_preview_view(false, Some("нет".into()));
    assert!(!off.available && off.note == "нет");
}

/// Preview run: Rexon stops it on a button and on a time ceiling (not an
/// error), exiting by itself with code 0 is not an error either; a crash is a
/// refusal with the last output lines; a chatty program does not stall on the pipe.
#[test]
fn preview_progress() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    let sh = |script: &str| vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()];
    let never = AtomicBool::new(false);
    // Time ceiling.
    let t = Instant::now();
    assert_eq!(plan::run_greeter_preview(&sh("sleep 30"), &never, Duration::from_millis(300)), Ok(()));
    assert!(t.elapsed() < Duration::from_secs(5));
    // The «Close preview» button.
    let stop = AtomicBool::new(true);
    let t = Instant::now();
    assert_eq!(plan::run_greeter_preview(&sh("sleep 30"), &stop, Duration::from_secs(60)), Ok(()));
    assert!(t.elapsed() < Duration::from_secs(5));
    // Ignores SIGTERM — gets killed.
    let t = Instant::now();
    assert_eq!(plan::run_greeter_preview(&sh("trap '' TERM; sleep 30"), &stop, Duration::from_secs(60)), Ok(()));
    assert!(t.elapsed() < Duration::from_secs(8));
    // Exited successfully by itself (mock PLM login).
    assert_eq!(plan::run_greeter_preview(&sh("exit 0"), &never, Duration::from_secs(10)), Ok(()));
    // Crashed: the last lines go into the refusal; 200 KiB of output do not hang it.
    let err = plan::run_greeter_preview(
        &sh("head -c 200000 /dev/zero | tr '\\0' 'x' >&2; echo >&2; echo 'qt.qpa: no platform plugin' >&2; exit 1"),
        &never,
        Duration::from_secs(20),
    )
    .unwrap_err();
    assert!(err.contains("Couldn't show the login screen preview") && err.contains("no platform plugin"), "{err}");
    let err = plan::run_greeter_preview(&["/nonexistent/greeter".to_string()], &never, Duration::from_secs(1)).unwrap_err();
    assert!(err.contains("Couldn't show"), "{err}");
}

/// Own copy of the background picture: placed with a stamp (what and when),
/// the former copy of the same kind goes away, «Restore background» removes
/// everything; foreign names in the stamp are not accepted. Preview: own —
/// from the copy, foreign — only if readable, otherwise an honest label.
#[test]
fn own_background_copy() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let root = std::env::temp_dir().join(format!("rexon-kept-bg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let src = root.join("Закат.png");
    std::fs::write(&src, b"\x89PNG\r\n\x1a\nkept").unwrap();
    let data = root.join("data").to_string_lossy().into_owned();
    let dir = plan::login_background_keep_dir(&data);
    assert!(dir.ends_with("/data/rexon/login-background"));
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let exists = |p: &str| std::path::Path::new(p).is_file();
    let path = plan::keep_login_background_in(&dir, plan::LOGIN_BG_PLM, &src.to_string_lossy(), "png", 1793500000).unwrap();
    assert!(path.ends_with("/plm.png"));
    let (got, kept) = plan::kept_login_background(&dir, plan::LOGIN_BG_PLM, &read, &exists).unwrap();
    assert_eq!(got, path);
    assert_eq!(kept, plan::KeptBackground { file: "plm.png".into(), source: src.to_string_lossy().into_owned(), set_at: 1793500000 });
    assert!(plan::kept_login_background(&dir, plan::LOGIN_BG_SDDM, &read, &exists).is_none(), "виды не путаются");
    // A new picture of another format replaces the former one.
    std::fs::write(&src, b"\xff\xd8\xffjpg").unwrap();
    plan::keep_login_background_in(&dir, plan::LOGIN_BG_PLM, &src.to_string_lossy(), "jpg", 1793500100).unwrap();
    assert!(!std::path::Path::new(&path).exists());
    assert!(plan::kept_login_background(&dir, plan::LOGIN_BG_PLM, &read, &exists).unwrap().0.ends_with("/plm.jpg"));
    // A stamp with a foreign file name is not a copy.
    std::fs::write(format!("{dir}/sddm.json"), r#"{"file":"../../../etc/passwd","source":"x","set_at":1}"#).unwrap();
    assert!(plan::kept_login_background(&dir, plan::LOGIN_BG_SDDM, &read, &exists).is_none());
    // «Restore background».
    plan::drop_login_background_in(&dir, plan::LOGIN_BG_PLM);
    assert!(plan::kept_login_background(&dir, plan::LOGIN_BG_PLM, &read, &exists).is_none());
    assert!(!std::path::Path::new(&format!("{dir}/plm.json")).exists());
    let _ = std::fs::remove_dir_all(&root);

    // Preview and label.
    let yes = |_: &str| true;
    let no = |_: &str| false;
    assert_eq!(
        plan::custom_background_preview(true, "/t/rexon-background.png", Some("/k/sddm.png"), &no),
        ("/k/sddm.png".to_string(), "Background: your picture".to_string())
    );
    assert_eq!(plan::custom_background_preview(true, "/t/rexon-background.png", None, &yes).0, "/t/rexon-background.png");
    let (img, state) = plan::custom_background_preview(true, "/t/rexon-background.png", None, &no);
    assert!(img.is_empty() && state.contains("no copy"), "{state}");
    let (img, state) = plan::custom_background_preview(false, "/x/mine.jpg", Some("/k/sddm.png"), &no);
    assert!(img.is_empty() && state.contains("set outside Rexon"), "{state}");
    assert_eq!(plan::custom_background_preview(false, "/x/mine.jpg", None, &yes).0, "/x/mine.jpg");
    // SDDM view: own background — preview from the copy.
    let bg = plan::SddmBackground {
        supported: true,
        custom: true,
        ours: true,
        image: "/usr/share/sddm/themes/breeze/rexon-background.png".into(),
        kept: "/k/sddm.png".into(),
        ..Default::default()
    };
    let v = plan::sddm_greeter_view(Some("breeze"), None, &bg, &plan::DesktopLook::default(), true, "");
    assert_eq!(v.background.image, "/k/sddm.png");
    let theirs = plan::SddmBackground { ours: false, kept: String::new(), image: "/srv/x.jpg".into(), ..bg };
    let v = plan::sddm_greeter_view(Some("breeze"), None, &theirs, &plan::DesktopLook::default(), true, "");
    assert!(v.background.image.is_empty() && v.background.state.contains("set outside Rexon"));
}
