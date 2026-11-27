//! Golden check of `session_splash.rs` (the login splash).
//!
//! `golden/session_splash_cases.json` is the golden data. It was once taken
//! read-only from the live `rexon.session_splash` by a one-off generator
//! (HOME/XDG in a temporary directory, subprocess/socket/X11 overlay mocked,
//! mpv/ffmpeg/systemctl/kwriteconfig6/qdbus and the live KWin were not started, paths
//! normalized to `/HOME` and `/usr/local/lib/rexon`). Since 02.10.2026 Python in
//! Rexon is only an archive: the snapshot is frozen and serves as Rust's OWN
//! golden data. Covered: constants/paths, the Steam splash scan (built-in +
//! inventory, names from localconfig.vdf), the config (reading/merging/
//! serializing), the hybrid (LnF theme, ksplashrc, argv of kwriteconfig6/systemctl/
//! ffmpeg, the systemd unit, the hybrid_supported decision), the KWin rule
//! (creation/migration/idempotency + the registration bugfix),
//! reconfigure candidates, xrandr parsing, mpv IPC (payloads and answer
//! parsing), sd_notify, rexon-splash-overlay candidates/argv, mpv argv of all
//! `play` branches, install/uninstall autostart routes, poster choice,
//! message texts.
//!
//! The places where Rust deliberately departed from the recorded behavior are ONLY
//! conscious fixes (see the module header), each pinned here by a separate
//! test.

mod i18n_pin;

use rexon_core::session_splash as ss;
use serde_json::{json, Value};

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/session_splash_cases.json")).unwrap()
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

fn opt_str(v: &Value) -> Option<&str> {
    v.as_str()
}

const HOME: &str = "/HOME";
const RUN: &str = "/HOME/run";
const VIDEO: &str = "/HOME/movie.webm";
const SOCK: &str = "/HOME/run/rexon-splash-mpv.sock";

// ============================================================================
// Constants and paths
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn constants_and_paths() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ss::config_path(None, HOME), s(&g["config_path"]));
    assert_eq!(ss::autostart_path(None, HOME), s(&g["autostart_path"]));
    assert_eq!(ss::KSPLASH_THEME_ID, s(&g["ksplash_theme_id"]));
    assert_eq!(ss::lnf_theme_dir(HOME), s(&g["lnf_theme_dir"]));
    assert_eq!(ss::SYSTEMD_UNIT_NAME, s(&g["systemd_unit_name"]));
    assert_eq!(ss::systemd_unit_path(None, HOME), s(&g["systemd_unit_path"]));
    assert_eq!(ss::KWIN_APP_ID, s(&g["kwin_app_id"]));
    assert_eq!(ss::runtime_dir(Some(RUN)), s(&g["runtime_dir"]));
    assert_eq!(ss::runtime_dir(None), "/tmp");
    assert_eq!(ss::runtime_dir(Some("")), "/tmp");
    assert_eq!(ss::steam_root_candidates(HOME)[0], s(&g["steam_root"]));
    assert_eq!(
        ss::steam_root_candidates(HOME),
        vec![
            "/HOME/.local/share/Steam".to_string(),
            "/HOME/.steam/steam".to_string(),
            "/HOME/.steam/root".to_string(),
        ]
    );
    for (key, name) in ss::BUILTIN_NAMES {
        // The name is a msgid; the Python golden value is Russian, we compare the translation from ru.po.
        assert_eq!(g["builtin_names"][*key], Value::from(rexon_core::i18n::tr(name)));
    }
    assert_eq!(
        ss::BUILTIN_NAMES.len(),
        g["builtin_names"].as_object().unwrap().len()
    );
    // derived runtime paths
    assert_eq!(ss::ipc_sock_path(RUN), SOCK);
    assert_eq!(ss::play_lock_path(RUN), "/HOME/run/rexon-session-splash.lock");
    assert_eq!(ss::runtime_poster_path(RUN), s(&g["poster_runtime"]));
    assert_eq!(ss::theme_poster_path(HOME), s(&g["poster_theme_hit"]));
    assert_eq!(ss::ksplashrc_path(None, HOME), "/HOME/.config/ksplashrc");
    assert_eq!(ss::ksplashrc_path(Some("/xdg"), HOME), "/xdg/ksplashrc");
    assert_eq!(ss::kwinrulesrc_path(None, HOME), "/HOME/.config/kwinrulesrc");
    assert_eq!(
        ss::systemd_wants_link(None, HOME),
        format!(
            "/HOME/.config/systemd/user/plasma-workspace.target.wants/{}",
            ss::SYSTEMD_UNIT_NAME
        )
    );
}

/// Fix #5: the user config root respects `$XDG_CONFIG_HOME`.
/// In the original only `_ksplashrc_get` knew the variable, the other paths
/// hardcoded `~/.config` — with a non-standard value the systemd unit and
/// `.desktop` went past the directories where they are looked for (the splash autostart
/// was silently not enabled), and the KWin rule was written past the live `kwinrulesrc`.
#[test]
fn fix_paths_honor_xdg_config_home() {
    const XDG: &str = "/xdg/cfg";
    assert_eq!(
        ss::config_path(Some(XDG), HOME),
        "/xdg/cfg/rexon/session_splash.json"
    );
    assert_eq!(
        ss::autostart_path(Some(XDG), HOME),
        "/xdg/cfg/autostart/rexon-session-splash.desktop"
    );
    assert_eq!(
        ss::systemd_unit_path(Some(XDG), HOME),
        format!("/xdg/cfg/systemd/user/{}", ss::SYSTEMD_UNIT_NAME)
    );
    assert_eq!(
        ss::systemd_wants_dir(Some(XDG), HOME),
        "/xdg/cfg/systemd/user/plasma-workspace.target.wants"
    );
    assert_eq!(
        ss::systemd_wants_link(Some(XDG), HOME),
        format!(
            "/xdg/cfg/systemd/user/plasma-workspace.target.wants/{}",
            ss::SYSTEMD_UNIT_NAME
        )
    );
    assert_eq!(ss::kwinrulesrc_path(Some(XDG), HOME), "/xdg/cfg/kwinrulesrc");
    assert_eq!(ss::ksplashrc_path(Some(XDG), HOME), "/xdg/cfg/ksplashrc");
    // An empty variable = unset (Python truthiness), a trailing slash
    // does not give a double separator — the root is shared with we_client::config_home.
    assert_eq!(ss::config_path(Some(""), HOME), ss::config_path(None, HOME));
    assert_eq!(
        ss::config_path(Some("/xdg/cfg/"), HOME),
        ss::config_path(Some(XDG), HOME)
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn launcher_and_unit_texts() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::launcher_exec(Some("/usr/local/bin/rexon"), s(&g["python_executable"])),
        s(&g["launcher_exec_rexon"])
    );
    // ⛔ A DIVERGENCE FROM THE GOLDEN DATA, NAMED OUT LOUD (16.09.2026, block H).
    // Without a launcher in `PATH` the original gave `launcher_exec_fallback` —
    // `<python> -m rexon.app --session-splash`. This line went into the
    // autostart `Exec=` and the unit's `ExecStart=`, where it runs without
    // a working directory, and the system interpreter's `sys.path` has no
    // `rexon` package: the splash silently did not show at login.
    // Now there is nothing to start with — so it is empty, and the caller refuses out loud
    // (`Splash::install_desktop_autostart` / `install_hybrid`).
    assert!(
        s(&g["launcher_exec_fallback"]).contains("-m rexon.app"),
        "в эталоне пропала запись прежнего (сломанного) ответа без лаунчера"
    );
    assert_eq!(ss::launcher_exec(None, s(&g["python_executable"])), "");
    assert_eq!(ss::launcher_exec(Some(""), s(&g["python_executable"])), "");
    assert_eq!(
        ss::systemd_unit_content(s(&g["launcher_exec_rexon"])),
        s(&g["systemd_unit_content"])
    );
    // Retaken 30.09.2026: `Comment` in English + `Comment[<code>]` from
    // the catalog (previously only Russian, without a language key).
    assert_eq!(
        i18n_pin::ru_only(&ss::autostart_desktop_content(s(&g["launcher_exec_rexon"]))),
        s(&g["autostart_desktop_content"])
    );
    assert_eq!(
        [true.into(), Value::from(ss::msg_autostart_enabled())],
        g["autostart_install_msg"].as_array().unwrap()[..]
    );
}

// ============================================================================
// Steam: names from VDF + splash scan
// ============================================================================

#[test]
fn vdf_names() {
    let g = fx();
    assert_eq!(
        ss::parse_startup_movie_names(s(&g["vdf_text"])),
        g["startup_movie_names"]
    );
    assert_eq!(
        ss::parse_startup_movie_names(s(&g["vdf_bad_json_text"])),
        g["vdf_bad_json_names"]
    );
    assert_eq!(
        ss::parse_startup_movie_names("\"X\"\n{\n\t\"other\"\t\"1\"\n}\n"),
        g["vdf_no_key_names"]
    );
    assert_eq!(ss::parse_startup_movie_names(""), json!({}));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn scan_startup_movies() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let names = ss::parse_startup_movie_names(s(&g["vdf_text"]));
    let builtin = vec![
        "deck_startup.webm".to_string(),
        "bigpicture_startup.webm".to_string(),
    ];
    // the listing is UNsorted — sorting happens inside, like sorted() in Python
    let inventory: Vec<String> = [
        "readme.txt",
        "weird.webm",
        "34806_abc123.webm",
        "noext",
        "99999_zz.webm",
        "777_x.mp4",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    let out = Value::from(ss::scan_movies(
        s(&g["steam_root"]),
        &builtin,
        &inventory,
        &names,
        &[],
    ));
    // Fix #3: for a file without "_" the id comes from the stem, not from the full name
    // with the extension (Python gave id/name "weird.webm").
    let mut expected = g["scan_movies_python"].clone();
    {
        let arr = expected.as_array_mut().unwrap();
        let weird = arr.last_mut().unwrap();
        assert_eq!(weird["id"], "weird.webm"); // a Python quirk in the golden data
        weird["id"] = "weird".into();
        weird["name"] = "Инвентарь · weird".into();
    }
    assert_eq!(out, expected);
}

// ============================================================================
// Config
// ============================================================================

#[test]
fn config_roundtrip() {
    let g = fx();
    let initial = r#"{"path": "/old.webm", "sound": false, "prev_ksplash_theme": "Breeze"}"#;
    assert_eq!(ss::parse_config(Some(initial)), g["load_config_ok"]);
    assert_eq!(ss::parse_config(Some("{broken")), g["load_config_broken"]);
    assert_eq!(ss::parse_config(Some("[1, 2]")), g["load_config_nondict"]);
    assert_eq!(ss::parse_config(None), g["load_config_missing"]);

    // save_config: updating path/sound without losing the service keys
    let cfg = ss::parse_config(Some(initial));
    let saved = ss::config_with_selection(&cfg, "/usr/local/share/movie.webm", false);
    assert_eq!(ss::config_json(&saved), s(&g["save_config_text"]));
    // save_hybrid_pref on top of what was saved
    let with_pref = ss::config_with_hybrid_pref(&saved, true);
    assert_eq!(ss::config_json(&with_pref), s(&g["save_hybrid_pref_text"]));
}

#[test]
fn hybrid_flags() {
    let installed = json!({"hybrid_installed": true});
    assert!(ss::hybrid_installed(&installed, true));
    assert!(!ss::hybrid_installed(&installed, false));
    assert!(!ss::hybrid_installed(&json!({}), true));
    assert!(ss::should_refresh_hybrid_poster(&installed, true));
    assert!(!ss::should_refresh_hybrid_poster(&installed, false));
    assert!(!ss::should_refresh_hybrid_poster(&json!({}), true));
}

// ============================================================================
// Hybrid: LnF theme, ksplashrc, argv, support decision
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn lnf_theme_texts() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ss::SPLASH_QML, s(&g["splash_qml"]));
    assert_eq!(ss::SPLASH_QML, s(&g["lnf_splash_qml"]));
    // Retaken 30.09.2026: the theme description in English + `Description[<code>]`
    // from the catalog (previously only Russian, without a language key).
    assert_eq!(i18n_pin::ru_only(&ss::lnf_metadata_json()), s(&g["lnf_metadata_json"]));
    assert_eq!(
        [true.into(), Value::from(ss::msg_lnf_updated())],
        g["write_lnf_msg"].as_array().unwrap()[..]
    );
}

#[test]
fn ffmpeg_poster() {
    let g = fx();
    assert_eq!(
        ss::poster_ffmpeg_argv("/usr/bin/ffmpeg", "/usr/local/share/movie.webm", "/HOME/poster.png"),
        str_vec(&g["ffmpeg_poster_argv"])
    );
}

#[test]
fn ksplashrc_read() {
    let g = fx();
    let text = s(&g["ksplashrc_text"]);
    for (key, expected) in g["ksplashrc_get"].as_object().unwrap() {
        assert_eq!(ss::ksplashrc_get(text, key), s(expected), "ksplashrc {key}");
    }
    assert_eq!(ss::ksplashrc_get("", "Theme"), s(&g["ksplashrc_get_nofile"]));
}

#[test]
fn kwriteconfig_and_systemctl_argv() {
    let g = fx();
    assert_eq!(
        ss::kwriteconfig_argv("/usr/bin/kwriteconfig6", "Theme", ss::KSPLASH_THEME_ID),
        str_vec(&g["kwriteconfig_set_argv"])
    );
    assert_eq!(
        ss::kwriteconfig_argv("/usr/bin/kwriteconfig6", "Engine", ""),
        str_vec(&g["kwriteconfig_delete_argv"])
    );
    assert_eq!(
        ss::systemctl_user_argv("/usr/bin/systemctl", &["is-active", "plasma-workspace.target"]),
        str_vec(&g["systemctl_is_active_argv"])
    );
}

#[test]
fn hybrid_supported_cases() {
    let g = fx();
    for case in g["hybrid_supported"].as_array().unwrap() {
        let desk = opt_str(&case["desktop"]);
        assert_eq!(
            ss::hybrid_supported_decision(
                desk,
                case["binaries"].as_bool().unwrap(),
                case["target_active"].as_bool().unwrap(),
            ),
            case["supported"].as_bool().unwrap(),
            "hybrid_supported {case}"
        );
    }
    assert_eq!(
        ss::HYBRID_REQUIRED_BINARIES,
        ["ffmpeg", "kwriteconfig6", "kreadconfig6", "ksplashqml", "systemctl"]
    );
}

#[test]
fn backup_rollback_decisions() {
    assert!(ss::should_backup_prev_theme("org.kde.breeze.desktop"));
    assert!(ss::should_backup_prev_theme("")); // the key did not exist — also a backup
    assert!(!ss::should_backup_prev_theme(ss::KSPLASH_THEME_ID));
    assert!(ss::should_rollback_ksplashrc(ss::KSPLASH_THEME_ID));
    assert!(!ss::should_rollback_ksplashrc("org.kde.breeze.desktop"));
}

// ============================================================================
// KWin rule
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_texts() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ss::KWIN_RULE_DESC, s(&g["kwin_rule_desc"]));
    assert_eq!(ss::KWIN_RULE_FSP, s(&g["kwin_rule_fsp"]));
    assert_eq!(ss::kwin_rule_body(), s(&g["kwin_rule_body"]));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_create_empty() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::ensure_kwin_rule_text(None).as_deref(),
        Some(s(&g["kwin_empty"]["result"]))
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_append_to_existing() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(s(&g["kwin_append"]["initial"]))).as_deref(),
        Some(s(&g["kwin_append"]["result"]))
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_uptodate_untouched() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    // an up-to-date rule already exists → the file is not rewritten
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(s(&g["kwin_uptodate"]["initial"]))),
        None
    );
    assert!(!g["kwin_uptodate"]["changed"].as_bool().unwrap());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_migrate_old() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(s(&g["kwin_migrate"]["initial"]))).as_deref(),
        Some(s(&g["kwin_migrate"]["result"]))
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_number_gaps() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(s(&g["kwin_gaps"]["initial"]))).as_deref(),
        Some(s(&g["kwin_gaps"]["result"]))
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_broken_general_fixed() {
    // The golden data was taken with the Russian rule name.
    let _lang = i18n_pin::ru();
    let g = fx();
    // Fix #1: [General] without rules=/count= — Python appended the block but did NOT
    // register it (re.sub idled → KWin ignores the rule); Rust
    // writes the missing keys right after the [General] header.
    let input = s(&g["kwin_broken_general_input"]);
    let fixed = format!(
        "[1]\nwmclass=a\n\n[General]\ncount=1\nrules=2\nsomething=1\n\n[2]\n{}",
        ss::kwin_rule_body()
    );
    let out = ss::ensure_kwin_rule_text(Some(input)).unwrap();
    assert_eq!(out, fixed);
    // ...and this differs from the golden (buggy) Python output.
    assert_ne!(out, s(&g["kwin_broken_general_python"]));
    assert!(!s(&g["kwin_broken_general_python"]).contains("rules="));
}

// ----------------------------------------------------------------------------
// The rule name is translated, and an existing rule is found even after a language
// change (01.10.2026). The name is visible to the person in KDE «Window Rules»; before
// the translation it was Russian for everyone.
// ----------------------------------------------------------------------------

/// The rule as Rexon writes it, with the given name.
fn our_rule(n: &str, title: &str, with_fsp: bool) -> String {
    let body = ss::kwin_rule_body();
    let tail = body.split_once('\n').unwrap().1; // everything after the Description= line
    let tail = if with_fsp { tail.to_string() } else { tail.replacen(ss::KWIN_RULE_FSP, "", 1) };
    format!("[{n}]\nDescription={title}\n{tail}")
}

const FOREIGN: &str = "[1]\nDescription=Firefox поверх всего\nabove=true\naboverule=2\nwmclass=firefox\nwmclassmatch=1\n\n";

fn general(rules: &str, count: usize) -> String {
    format!("\n[General]\ncount={count}\nrules={rules}\n")
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_title_follows_language() {
    {
        let _lang = i18n_pin::en();
        assert_eq!(ss::kwin_rule_desc_line(), "Description=Rexon: startup movie on top of everything\n");
        assert!(ss::kwin_rule_body().starts_with("Description=Rexon: startup movie on top of everything\nfsplevel=0\n"));
    }
    let _lang = i18n_pin::ru();
    assert_eq!(ss::kwin_rule_desc_line(), ss::KWIN_RULE_DESC, "по-русски — как писал оригинал");
}

/// An old Russian rule of a person who switched to English:
/// it is found (no second rule appears) and renamed.
#[test]
fn kwin_rule_old_russian_found_and_renamed() {
    let _lang = i18n_pin::en();
    let en = "Rexon: startup movie on top of everything";
    let ru = "Rexon: заставка входа поверх всего";
    let before = format!("{FOREIGN}{}{}", our_rule("2", ru, true), general("1,2", 2));
    let after = ss::ensure_kwin_rule_text(Some(&before)).expect("название переписывается");
    assert_eq!(after, format!("{FOREIGN}{}{}", our_rule("2", en, true), general("1,2", 2)));
    // A repeat run changes nothing — the file is not rewritten every time.
    assert_eq!(ss::ensure_kwin_rule_text(Some(&after)), None);

    // A very old Russian rule without the FSP removal: both edits at once.
    let old = format!("{}{}", our_rule("1", ru, false), general("1", 1));
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(&old)).unwrap(),
        format!("{}{}", our_rule("1", en, true), general("1", 1))
    );
}

/// A rule with an English name for a person with the Russian language.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_english_found_under_russian() {
    let _lang = i18n_pin::ru();
    let en = "Rexon: startup movie on top of everything";
    let before = format!("{}{}", our_rule("7", en, true), general("7", 1));
    let after = ss::ensure_kwin_rule_text(Some(&before)).unwrap();
    assert_eq!(after, format!("{}{}", our_rule("7", "Rexon: заставка входа поверх всего", true), general("7", 1)));
    assert_eq!(ss::ensure_kwin_rule_text(Some(&after)), None);
}

/// The German name comes from the REAL `po/de.po` (built by msgfmt): the rule
/// is found by the translation even without `wmclass` (edited by hand), no second
/// block appears.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/de.po настоящим msgfmt")]
fn kwin_rule_german_found_by_title() {
    let dir = std::env::temp_dir().join(format!("rexon-kwin-de-{}", std::process::id()));
    let mo = rexon_core::i18n::catalog_file(&dir, "de", "mo");
    std::fs::create_dir_all(mo.parent().unwrap()).unwrap();
    let po = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../po/de.po");
    let ok = std::process::Command::new("msgfmt").arg("-o").arg(&mo).arg(&po).status().unwrap();
    assert!(ok.success(), "msgfmt отверг po/de.po");
    let de = rexon_core::i18n::translations_in(ss::KWIN_RULE_TITLE, &["de".to_string()], &[dir.clone()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(de.len(), 1, "в po/de.po нет перевода названия правила");
    let de = de[0].clone();
    assert!(!de.contains("Rexon: startup"), "немецкое название не переведено: {de}");

    let mut known = vec![ss::KWIN_RULE_TITLE.to_string()];
    known.push(de.clone());
    let en = ss::KWIN_RULE_TITLE;
    // wmclass removed by hand — we recognize it by the name.
    let rule = our_rule("3", &de, true).replace("wmclass=rexon-session-splash\nwmclassmatch=1\n", "");
    let before = format!("{FOREIGN}{rule}{}", general("1,3", 2));
    let after = ss::ensure_kwin_rule_text_with(Some(&before), en, &known).unwrap();
    assert_eq!(after, before.replace(&format!("Description={de}\n"), &format!("Description={en}\n")));
    assert_eq!(after.matches("[General]").count(), 1);
    assert_eq!(after.matches("\n[").count(), before.matches("\n[").count(), "второго правила нет");
}

/// Other rules are not touched: neither a similar `wmclass` nor a custom name
/// the person gave to our rule.
#[test]
fn kwin_rule_foreign_untouched() {
    let _lang = i18n_pin::en();
    let en = "Rexon: startup movie on top of everything";
    // `wmclass=rexon-session-splash-x` is not ours: previously it would have matched as a substring.
    let lookalike = "[2]\nDescription=Чужое\nwmclass=rexon-session-splash-x\nwmclassmatch=1\n";
    let before = format!("{FOREIGN}{lookalike}{}", general("1,2", 2));
    let after = ss::ensure_kwin_rule_text(Some(&before)).expect("наше правило добавляется");
    assert!(after.starts_with(&format!("{FOREIGN}{lookalike}")), "чужие секции изменены:\n{after}");
    assert!(after.contains(&format!("Description={en}\n")));

    // Our rule renamed by the person: the name is theirs, we do not touch it.
    let mine = format!("{}{}", our_rule("1", "Моя заставка", true), general("1", 1));
    assert_eq!(ss::ensure_kwin_rule_text(Some(&mine)), None);
}

#[test]
fn kwin_reconfigure_cmds() {
    let g = fx();
    let expected: Vec<Vec<String>> = g["kwin_reconfigure_argvs"]
        .as_array()
        .unwrap()
        .iter()
        .map(str_vec)
        .collect();
    assert_eq!(ss::kwin_reconfigure_candidates(), expected);
}

// ============================================================================
// mpv IPC / sd_notify
// ============================================================================

#[test]
fn ipc_payloads() {
    let g = fx();
    assert_eq!(
        ss::mpv_ipc_command_payload(&json!(["set_property", "pause", false])),
        s(&g["ipc_send_pause_false"])
    );
    assert_eq!(
        ss::mpv_ipc_command_payload(&json!(["quit"])),
        s(&g["ipc_send_quit"])
    );
}

#[test]
fn ipc_get_roundtrip() {
    let g = fx();
    for case in g["ipc_get"].as_array().unwrap() {
        let label = s(&case["label"]);
        assert_eq!(
            ss::mpv_ipc_get_payload("eof-reached"),
            s(&case["sent"]),
            "sent {label}"
        );
        let expected = if case["result"].is_null() {
            None
        } else {
            Some(case["result"].clone())
        };
        assert_eq!(
            ss::parse_mpv_ipc_response(s(&case["received"]), ss::MPV_IPC_REQUEST_ID),
            expected,
            "result {label}"
        );
    }
}

#[test]
fn ipc_get_non_object_line_fixed() {
    // Fix #2: valid JSON that is not an object in the stream crashed Python
    // (AttributeError past `except OSError`); here the line is skipped.
    let received = "[1, 2]\n{\"request_id\":700,\"error\":\"success\",\"data\":1}\n";
    assert_eq!(
        ss::parse_mpv_ipc_response(received, ss::MPV_IPC_REQUEST_ID),
        Some(json!(1))
    );
}

#[test]
fn sd_notify() {
    let g = fx();
    assert_eq!(
        ss::notify_socket_addr(Some("@/rexon-notify")).as_deref(),
        Some(s(&g["sd_notify_abstract_addr"]))
    );
    assert_eq!(
        ss::notify_socket_addr(Some("/run/systemd/notify")).as_deref(),
        Some(s(&g["sd_notify_plain_addr"]))
    );
    assert_eq!(ss::notify_socket_addr(None), None);
    assert_eq!(ss::notify_socket_addr(Some("")), None);
    assert_eq!(g["sd_notify_connect_count"], json!(2));
    for payload in g["sd_notify_payloads"].as_array().unwrap() {
        assert_eq!(s(payload), ss::SD_NOTIFY_READY);
    }
}

// ============================================================================
// X11: xrandr, session type
// ============================================================================

#[test]
fn xrandr_geometry() {
    let g = fx();
    for case in g["xrandr"].as_array().unwrap() {
        let expected = case["geometry"].as_array().map(|a| {
            (
                a[0].as_i64().unwrap(),
                a[1].as_i64().unwrap(),
                a[2].as_i64().unwrap(),
                a[3].as_i64().unwrap(),
            )
        });
        assert_eq!(
            ss::parse_xrandr_geometry(s(&case["stdout"])),
            expected,
            "xrandr {}",
            s(&case["label"])
        );
    }
    assert_eq!(
        ss::xrandr_query_argv("/usr/bin/xrandr"),
        vec!["/usr/bin/xrandr".to_string(), "--query".to_string()]
    );
}

#[test]
fn x11_session_detection() {
    let g = fx();
    for case in g["is_x11_session"].as_array().unwrap() {
        assert_eq!(
            ss::is_x11_session(opt_str(&case["display"]), opt_str(&case["wayland"])),
            case["x11"].as_bool().unwrap(),
            "is_x11_session {case}"
        );
    }
}

// ============================================================================
// Run: helper, mpv argv, messages
// ============================================================================

#[test]
fn overlay_binary_candidates() {
    let g = fx();
    assert_eq!(
        ss::splash_overlay_candidates("/usr/local/lib/rexon"),
        str_vec(&g["overlay_candidates"])
    );
    assert!(g["overlay_not_found"].is_null());
}

#[test]
fn overlay_helper_argv() {
    let g = fx();
    assert_eq!(
        ss::splash_overlay_argv("/usr/local/bin/rexon-splash-overlay", VIDEO, SOCK, false, true),
        str_vec(&g["play"]["helper_argv_mute_paused"]["argv"])
    );
    assert_eq!(
        ss::splash_overlay_argv("/usr/local/bin/rexon-splash-overlay", VIDEO, SOCK, true, false),
        str_vec(&g["play"]["helper_argv_sound"]["argv"])
    );
}

#[test]
fn mpv_argv_wayland_window() {
    let g = fx();
    assert_eq!(
        ss::mpv_argv(VIDEO, true, false, &ss::MpvTarget::Window { wayland: true }, SOCK),
        str_vec(&g["play"]["wayland_window_test"]["argv"])
    );
    assert_eq!(
        ss::mpv_argv(VIDEO, false, true, &ss::MpvTarget::Window { wayland: true }, SOCK),
        str_vec(&g["play"]["wayland_window_hybrid_mute"]["argv"])
    );
}

#[test]
fn mpv_argv_x11_window() {
    let g = fx();
    assert_eq!(
        ss::mpv_argv(VIDEO, true, false, &ss::MpvTarget::Window { wayland: false }, SOCK),
        str_vec(&g["play"]["x11_window_test"]["argv"])
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn mpv_argv_x11_overlay() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        ss::mpv_argv(
            VIDEO,
            true,
            false,
            &ss::MpvTarget::X11Overlay { window_id: 77000001 },
            SOCK
        ),
        str_vec(&g["play"]["x11_overlay_blocking"]["argv"])
    );
    assert_eq!(
        g["play"]["x11_overlay_blocking"]["result"],
        json!([true, ss::msg_played()])
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn helper_fallback_chain() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let case = &g["play"]["wayland_helper_then_fallback"];
    // the helper starts without --mute/--paused (sound on, hybrid auto-off)
    assert_eq!(
        ss::splash_overlay_argv("/usr/local/bin/rexon-splash-overlay", VIDEO, SOCK, true, false),
        str_vec(&case["helper_argv"])
    );
    // died before the IPC socket → fallback to windowed mpv (a Wayland window, no hybrid)
    assert_eq!(
        ss::mpv_argv(VIDEO, true, false, &ss::MpvTarget::Window { wayland: true }, SOCK),
        str_vec(&case["mpv_argv"])
    );
    assert_eq!(case["result"], json!([true, ss::msg_played()]));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn play_messages() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(g["play"]["no_file"]["result"], json!([false, ss::msg_no_file()]));
    assert_eq!(g["play"]["no_mpv"]["result"], json!([false, ss::msg_no_mpv()]));
    assert_eq!(
        g["play"]["wayland_window_test"]["result"],
        json!([true, ss::msg_launched()])
    );
    assert_eq!(ss::mpv_exit_message(3), "mpv вернул код 3");
    assert_eq!(ss::helper_exit_message(1), "rexon-splash-overlay вернул код 1");
}

#[test]
fn play_route_decisions() {
    // effective_hybrid: None → auto (hybrid installed AND blocking)
    assert!(ss::effective_hybrid(None, true, true));
    assert!(!ss::effective_hybrid(None, true, false));
    assert!(!ss::effective_hybrid(None, false, true));
    assert!(!ss::effective_hybrid(Some(false), true, true));
    assert!(ss::effective_hybrid(Some(true), false, false));
    // the helper — only a real run on Wayland
    assert!(ss::wants_wayland_helper(true, Some(":0"), Some("wayland-0")));
    assert!(ss::wants_wayland_helper(true, None, Some("wayland-0")));
    assert!(!ss::wants_wayland_helper(false, None, Some("wayland-0")));
    assert!(!ss::wants_wayland_helper(true, Some(":0"), None));
    assert!(!ss::wants_wayland_helper(true, Some(":0"), Some("")));
    // is_enabled: either of the two mechanisms
    assert!(ss::is_enabled(true, false));
    assert!(ss::is_enabled(false, true));
    assert!(!ss::is_enabled(false, false));
}

// ============================================================================
// Autostart: routes and messages
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn autostart_routes() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let r = &g["autostart_routes"];
    // no path → refusal
    assert_eq!(ss::autostart_route(&json!({}), true), ss::AutostartRoute::NoPath);
    assert_eq!(r["no_path"]["result"], json!([false, ss::msg_choose_first()]));
    // a path exists, the checkbox at its default, hybrid supported → hybrid
    let cfg = json!({"path": VIDEO, "sound": true});
    assert_eq!(ss::autostart_route(&cfg, true), ss::AutostartRoute::Hybrid);
    assert_eq!(r["hybrid_ok"]["result"], json!([true, ss::msg_hybrid_route_ok()]));
    assert_eq!(
        r["hybrid_ok"]["calls"],
        json!(["supported", "install_hybrid", "rm_desktop"])
    );
    // hybrid failed → rollback + phase 1 with a compound message
    assert_eq!(
        r["hybrid_fail_fallback"]["result"],
        json!([
            true,
            ss::hybrid_fallback_message("нет ffmpeg", true, &ss::msg_autostart_enabled())
        ])
    );
    assert_eq!(
        ss::hybrid_fallback_message("x", false, "не удалось"),
        "не удалось"
    );
    assert_eq!(
        r["hybrid_fail_fallback"]["calls"],
        json!(["supported", "install_hybrid", "uninstall_hybrid", "desktop"])
    );
    // checkbox off + hybrid leftovers → a clean phase 1 with rollback
    let cfg2 = json!({"path": VIDEO, "hybrid": false, "hybrid_installed": true});
    assert_eq!(
        ss::autostart_route(&cfg2, true),
        ss::AutostartRoute::Plain { rollback_hybrid: true }
    );
    assert_eq!(
        r["hybrid_disabled_leftovers"]["result"],
        json!([true, ss::msg_autostart_enabled()])
    );
    assert_eq!(
        r["hybrid_disabled_leftovers"]["calls"],
        json!(["uninstall_hybrid", "desktop"])
    );
    // hybrid not supported, no leftovers → just phase 1
    assert_eq!(
        ss::autostart_route(&cfg, false),
        ss::AutostartRoute::Plain { rollback_hybrid: false }
    );
    // uninstall: one message for both paths
    assert_eq!(
        r["uninstall_with_hybrid"]["result"],
        json!([true, ss::msg_autostart_disabled()])
    );
    assert_eq!(r["uninstall_with_hybrid"]["calls"], json!(["uninstall_hybrid"]));
    assert_eq!(
        r["uninstall_plain"]["result"],
        json!([true, ss::msg_autostart_disabled()])
    );
}

// ============================================================================
// Poster choice
// ============================================================================

#[test]
fn poster_selection() {
    let g = fx();
    let cfg = json!({"path": VIDEO, "sound": true});
    // video from the config + the theme poster in place → the theme's ready poster
    assert_eq!(
        ss::poster_source(&cfg, VIDEO, true, HOME, RUN),
        ss::PosterSource::Theme(s(&g["poster_theme_hit"]).to_string())
    );
    // no theme → a fresh frame by ffmpeg into runtime
    assert_eq!(
        ss::poster_source(&cfg, VIDEO, false, HOME, RUN),
        ss::PosterSource::Extract(s(&g["poster_runtime"]).to_string())
    );
    // another video → extraction as well, even with a live theme poster
    assert_eq!(
        ss::poster_source(&cfg, "/HOME/other.webm", true, HOME, RUN),
        ss::PosterSource::Extract(s(&g["poster_runtime"]).to_string())
    );
    assert!(g["poster_fail"].is_null()); // ffmpeg failure → None (executor)
}

// ============================================================================
// Splash showcase: item metadata, frame, favorites, rotation
// ============================================================================

fn sc() -> Value {
    fx()["showcase"].clone()
}

fn hex_bytes(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn startup_movie_items_full_records() {
    let g = sc();
    // The record is returned IN FULL (cover, item_class, localized
    // title) — the showcase needs all the fields, unlike the scan route.
    assert_eq!(
        ss::parse_startup_movie_items(s(&g["vdf_text"])),
        g["items"]
    );
    // A numeric communityitemid becomes a string key, like str() in Python.
    let items = ss::parse_startup_movie_items(s(&g["vdf_text"]));
    assert!(items.get("991").is_some());
    // Dropped: null, "none" and an element that is not a dict.
    assert_eq!(items.as_object().unwrap().len(), 3);
    assert!(items.get("None").is_none());
    assert!(items.get("none").is_none());
    // No GetStartupMovies key / broken JSON → empty.
    assert_eq!(ss::parse_startup_movie_items("\"Other\"\t\"1\"\n"), json!({}));
    assert_eq!(
        ss::parse_startup_movie_items("\"GetStartupMovies\"\t\"[не json\"\n"),
        json!({})
    );
}

#[test]
fn appinfo_names_binary_vdf() {
    let g = sc();
    // The version with a string table (0x07564429) is the one that really lies in
    // a modern Steam client.
    let want: Vec<i64> = vec![1_259_420, 570, 999];
    for tag in ["v29", "v28"] {
        let data = hex_bytes(&g[format!("appinfo_{tag}_bytes")]);
        let got = ss::parse_appinfo_names(&data, &want);
        let expect: Vec<(i64, String)> = g[format!("appinfo_{tag}_names")]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.parse::<i64>().unwrap(), v.as_str().unwrap().to_string()))
            .collect();
        assert_eq!(got, expect, "версия {tag}");
    }
    // An unknown version → empty (in Python too, with a log entry).
    let bad = hex_bytes(&g["appinfo_bad_bytes"]);
    assert!(ss::parse_appinfo_names(&bad, &want).is_empty());
    // An empty request does not read the file at all.
    let data = hex_bytes(&g["appinfo_v29_bytes"]);
    assert!(ss::parse_appinfo_names(&data, &[]).is_empty());
    assert!(ss::parse_appinfo_names(&data, &[0]).is_empty());
    // A truncated file does not panic (Python caught struct.error).
    for cut in [0, 4, 15, 16, 20, 40] {
        let _ = ss::parse_appinfo_names(&data[..cut.min(data.len())], &want);
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn enrich_movies_showcase_fields() {
    let _lang = i18n_pin::ru();
    let g = sc();
    let movies = g["enrich_input_movies"].as_array().unwrap().clone();
    let names: Vec<(i64, String)> = g["enrich_input_names"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse::<i64>().unwrap(), v.as_str().unwrap().to_string()))
        .collect();
    let got = ss::enrich_movies(&movies, &g["items"], &names);
    assert_eq!(Value::Array(got.clone()), g["enriched"]);
    // The key showcase rules are pinned separately from the general comparison:
    // the localized item_title beats the English name (rule #13),
    // the cover's leading slash is stripped, appid arrives both as a string and as a number.
    assert_eq!(got[1]["title"], json!("Специальная доставка"));
    assert_eq!(got[1]["image"], json!("items/1259420/big.png"));
    assert_eq!(got[1]["game"], json!("Days Gone"));
    assert_eq!(got[2]["appid"], json!(570)); // in the VDF it was "570" as a string
    assert_eq!(got[2]["title"], json!("Only Name")); // no item_title → name
    assert_eq!(got[0]["origin"], json!(ss::ORIGIN_BUILTIN));
    assert_eq!(got[4]["appid"], json!(0)); // no metadata at all
    assert_eq!(got[4]["title"], json!("Без метаданных")); // the scan name remains
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn describe_custom_record() {
    let _lang = i18n_pin::ru();
    let g = sc();
    for (path, expect) in g["describe_custom"].as_object().unwrap() {
        let base = path.rsplit('/').next().unwrap();
        let stem = match base.rfind('.') {
            Some(p) if p > 0 => &base[..p],
            _ => base,
        };
        assert_eq!(&ss::describe_custom(path, base, stem), expect, "{path}");
    }
    // Origin captions are the same strings as in Python.
    let labels = &g["origin_labels"];
    for id in [ss::ORIGIN_BUILTIN, ss::ORIGIN_SHOP, ss::ORIGIN_CUSTOM] {
        assert_eq!(ss::origin_label(id), s(&labels[id]));
    }
    assert_eq!(
        str_vec(&g["origins"]),
        vec![ss::ORIGIN_BUILTIN, ss::ORIGIN_SHOP, ss::ORIGIN_CUSTOM]
    );
    // Unknown origin → an empty caption (we do not draw an empty label).
    assert_eq!(ss::origin_label("xyz"), "");
}

#[test]
fn favorites_toggle() {
    let g = sc();
    for (tag, case) in g["favorites"].as_object().unwrap() {
        let (cfg, fav) = ss::config_with_favorite(
            &case["input"],
            match tag.as_str() {
                "add_first" | "add_dup" => "/a.webm",
                "remove" => "/a.webm",
                "remove_absent" => "/zz.webm",
                _ => "/b.webm",
            },
            !tag.starts_with("remove"),
        );
        assert_eq!(fav, str_vec(&case["list"]), "{tag}: список");
        assert_eq!(cfg["favorites"], case["config"]["favorites"], "{tag}: конфиг");
    }
    // Non-strings in favorites are dropped, like Python's isinstance filter.
    assert_eq!(
        ss::favorites_from_config(&json!({"favorites": ["/a", 5, null, "/b"]})),
        vec!["/a".to_string(), "/b".to_string()]
    );
    // Not a list, or the key is missing → empty.
    assert!(ss::favorites_from_config(&json!({"favorites": "нет"})).is_empty());
    assert!(ss::favorites_from_config(&json!({})).is_empty());
}

#[test]
fn rotation_rules() {
    let g = sc();
    for (tag, expect) in g["rotation_enabled"].as_object().unwrap() {
        let cfg = match tag.as_str() {
            "absent" => json!({}),
            "true" => json!({"rotate": true}),
            "false" => json!({"rotate": false}),
            "truthy_str" => json!({"rotate": "y"}),
            _ => json!({"rotate": 0}),
        };
        assert_eq!(ss::rotation_enabled(&cfg), expect.as_bool().unwrap(), "{tag}");
    }
    assert_eq!(
        ss::config_with_rotation(&json!({"path": "/p"}), true),
        g["set_rotation"]["on"]
    );
    assert_eq!(
        ss::config_with_rotation(&json!({"rotate": true}), false),
        g["set_rotation"]["off"]
    );

    // The pool: favorites from two items up, otherwise everything found (rule #13 —
    // a single star must not give "the same video at every login").
    let p = &g["rotation_pool"];
    assert_eq!(
        ss::rotation_pool(&["/a".into(), "/b".into()], &["/a".into(), "/b".into(), "/c".into()]),
        str_vec(&p["fav_two"])
    );
    assert_eq!(
        ss::rotation_pool(&["/a".into()], &["/a".into(), "/b".into(), "/c".into()]),
        str_vec(&p["fav_one"])
    );
    assert_eq!(ss::rotation_pool(&[], &["/a".into()]), str_vec(&p["fav_none"]));
    assert_eq!(ss::rotation_pool(&["/a".into()], &[]), str_vec(&p["all_empty"]));

    // Draw candidates: without the current one, and if empty after filtering — the whole pool.
    let c = &g["rotation_choices"];
    assert_eq!(
        ss::rotation_choices(&["/a".into(), "/b".into(), "/c".into()], Some("/b")),
        str_vec(&c["skip_current"])
    );
    assert_eq!(
        ss::rotation_choices(&["/a".into(), "/b".into()], Some("/zz")),
        str_vec(&c["current_absent"])
    );
    assert_eq!(
        ss::rotation_choices(&["/a".into(), "/a".into()], Some("/a")),
        str_vec(&c["all_same"])
    );
    // A pool smaller than two → nothing to rotate (in Python this is an early return None).
    assert!(ss::rotation_choices(&["/a".into()], None).is_empty());
    assert!(ss::rotation_choices(&[], None).is_empty());
}

#[test]
fn preview_frame_argv_and_cache() {
    let g = sc();
    assert_eq!(ss::PREVIEW_AT, g["preview_at"].as_f64().unwrap());
    assert_eq!(
        ss::ffprobe_duration_argv(s(&g["ffprobe_exe"]), VIDEO),
        str_vec(&g["ffprobe_argv"])
    );
    for (tag, expect) in g["parse_duration"].as_object().unwrap() {
        let text = match tag.as_str() {
            "plain" => "12.5\n",
            "int" => "7\n",
            "empty" => "",
            "junk" => "N/A\n",
            "negative" => "-3\n",
            _ => "  4.25  ",
        };
        assert_eq!(ss::parse_duration(text), expect.as_f64().unwrap(), "{tag}");
    }
    let a = &g["preview_argv"];
    let exe = "/usr/bin/ffmpeg";
    let tmp = "/HOME/out.png.part.png";
    assert_eq!(
        ss::preview_ffmpeg_argv(exe, VIDEO, tmp, 9.375),
        str_vec(&a["seek"])
    );
    // seek ≤ 0.05 → without -ss (the same argv as the retry).
    assert_eq!(
        ss::preview_ffmpeg_argv(exe, VIDEO, tmp, 0.0),
        str_vec(&a["no_seek"])
    );
    assert_eq!(
        ss::preview_ffmpeg_argv(exe, VIDEO, tmp, 0.04),
        str_vec(&a["tiny_seek"])
    );

    // The cache directory respects $XDG_CACHE_HOME.
    let rel = s(&g["preview_cache_dir_rel"]);
    assert_eq!(ss::preview_cache_dir(None, HOME), format!("/HOME/.cache/{rel}"));
    assert_eq!(
        ss::preview_cache_dir(Some("/tmp/c"), HOME),
        format!("/tmp/c/{rel}")
    );
    assert_eq!(ss::preview_cache_dir(Some(""), HOME), format!("/HOME/.cache/{rel}"));
}

#[test]
fn preview_cache_key_is_stable() {
    // ⚠️ Fix #6: the fingerprint in the cache file name is DETERMINISTIC.
    // Python's `hash((abspath, mtime))` is salted by PYTHONHASHSEED, so
    // byte parity with the original CANNOT exist here — the golden value is unstable
    // between runs. We compare what remains of the original (the name
    // sanitizer) and the main property of the fix: the same input → the same name,
    // a different mtime/path → a different one.
    let g = sc();
    let dir = "/HOME/.cache/rexon/steam_splash_preview";
    for (path, key) in g["preview_keys"].as_object().unwrap() {
        let got = ss::preview_cache_path(dir, path, 100);
        let key = s(key);
        assert!(
            got.starts_with(&format!("{dir}/{key}.")),
            "{path}: {got} не начинается с {dir}/{key}."
        );
        assert!(got.ends_with(".png"), "{path}: {got}");
    }
    let a = ss::preview_cache_path(dir, "/HOME/movie.webm", 100);
    assert_eq!(a, ss::preview_cache_path(dir, "/HOME/movie.webm", 100));
    assert_ne!(a, ss::preview_cache_path(dir, "/HOME/movie.webm", 101));
    assert_ne!(a, ss::preview_cache_path(dir, "/HOME/other.webm", 100));
}

// ============================================================================
// Splash flows: replaying the executor log (the `SplashIo` seam)
// ============================================================================
//
// The second layer of the port: the core drives THE FLOWS THEMSELVES (scan, config, hybrid, autostart
// and all three run paths), and the side effects are executed by a trait. Here the trait
// is replaced by a logging stub: the ORDER of steps and the decisions at
// forks are checked — what used to live in Python `play`/`_install_hybrid` and was not
// covered by any check (the recorded golden data of the Python path did not touch them: you cannot start a live
// mpv/ffmpeg/systemctl in a test).
//
// `sleep` advances a fake clock — otherwise waits with deadlines would never end.

use rexon_core::session_splash::{RunOut, Splash, SplashIo};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct FakeIo {
    env: HashMap<String, String>,
    bins: HashMap<String, String>,
    files: HashSet<String>,
    dirs: HashSet<String>,
    texts: HashMap<String, String>,
    /// argv substring → (exit code, stdout, stderr)
    runs: Vec<(String, i64, String, String)>,
    written: HashMap<String, String>,
    journal: Vec<String>,
    now: f64,
    /// on which poll the process counts as finished
    exit_after_polls: Option<usize>,
    exit_code: i64,
    polls: usize,
    overlay: bool,
    compositor: bool,
    spawn_err: Option<String>,
    ipc_reply: Option<String>,
    lock_free: bool,
    steps: HashMap<String, (bool, String)>,
    /// The product's chosen entry; `None` — the Python entry (earlier checks).
    entry: Option<rexon_core::installer::LaunchEntry>,
}

impl FakeIo {
    fn new() -> Self {
        Self {
            now: 100.0,
            exit_code: 0,
            lock_free: true,
            ..Default::default()
        }
    }
    fn with_file(mut self, path: &str) -> Self {
        self.files.insert(path.to_string());
        self
    }
    fn with_dir(mut self, path: &str) -> Self {
        self.dirs.insert(path.to_string());
        self
    }
    fn with_bin(mut self, name: &str) -> Self {
        self.bins
            .insert(name.to_string(), format!("/usr/bin/{name}"));
        self
    }
    fn with_env(mut self, key: &str, val: &str) -> Self {
        self.env.insert(key.to_string(), val.to_string());
        self
    }
    fn with_run(mut self, needle: &str, rc: i64, out: &str, err: &str) -> Self {
        self.runs
            .push((needle.to_string(), rc, out.to_string(), err.to_string()));
        self
    }
    fn note(&mut self, s: String) {
        self.journal.push(s);
    }
    fn steps_of(&self, prefix: &str) -> Vec<String> {
        self.journal
            .iter()
            .filter(|l| l.starts_with(prefix))
            .cloned()
            .collect()
    }
    fn has(&self, needle: &str) -> bool {
        self.journal.iter().any(|l| l.contains(needle))
    }
    fn index_of(&self, needle: &str) -> usize {
        self.journal
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("нет шага «{needle}» в журнале: {:#?}", self.journal))
    }
}

impl SplashIo for FakeIo {
    fn env(&mut self, name: &str) -> String {
        self.env.get(name).cloned().unwrap_or_default()
    }
    fn which(&mut self, name: &str) -> String {
        self.bins.get(name).cloned().unwrap_or_default()
    }
    fn launch_entry(&mut self) -> rexon_core::installer::LaunchEntry {
        self.entry
            .clone()
            .unwrap_or(rexon_core::installer::LaunchEntry::Python)
    }
    fn is_file(&mut self, path: &str) -> bool {
        self.files.contains(path)
    }
    fn is_dir(&mut self, path: &str) -> bool {
        self.dirs.contains(path)
    }
    fn exists(&mut self, path: &str) -> bool {
        self.files.contains(path) || self.dirs.contains(path)
    }
    fn is_link(&mut self, path: &str) -> bool {
        self.files.contains(path)
    }
    fn is_executable(&mut self, path: &str) -> bool {
        self.files.contains(path)
    }
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn abspath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn listdir(&mut self, path: &str) -> Vec<String> {
        let pref = format!("{path}/");
        let mut out: Vec<String> = self
            .files
            .iter()
            .chain(self.dirs.iter())
            .filter_map(|f| f.strip_prefix(&pref).map(str::to_string))
            .filter(|rest| !rest.contains('/'))
            .collect();
        out.sort();
        out
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.texts.get(path).cloned()
    }
    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        self.texts.get(path).map(|t| t.as_bytes().to_vec())
    }
    fn mtime(&mut self, _path: &str) -> Option<i64> {
        Some(7)
    }
    fn file_size(&mut self, _path: &str) -> i64 {
        1
    }
    fn makedirs(&mut self, path: &str) -> Option<String> {
        self.note(format!("makedirs {path}"));
        self.dirs.insert(path.to_string());
        None
    }
    fn write_text(&mut self, path: &str, content: &str) -> Option<String> {
        self.note(format!("write {path}"));
        self.files.insert(path.to_string());
        self.texts.insert(path.to_string(), content.to_string());
        self.written.insert(path.to_string(), content.to_string());
        None
    }
    fn write_atomic(&mut self, path: &str, content: &str) -> Option<String> {
        self.note(format!("write_atomic {path}"));
        self.files.insert(path.to_string());
        self.texts.insert(path.to_string(), content.to_string());
        self.written.insert(path.to_string(), content.to_string());
        None
    }
    fn replace(&mut self, src: &str, dst: &str) -> Option<String> {
        self.note(format!("replace {src} → {dst}"));
        self.files.remove(src);
        self.files.insert(dst.to_string());
        None
    }
    fn remove_file(&mut self, path: &str) -> Option<String> {
        self.note(format!("rm {path}"));
        self.files.remove(path);
        None
    }
    fn remove_tree(&mut self, path: &str) -> Option<String> {
        self.note(format!("rmtree {path}"));
        self.dirs.remove(path);
        None
    }
    fn copy_file(&mut self, src: &str, dst: &str) -> Option<String> {
        self.note(format!("copy {src} → {dst}"));
        self.files.insert(dst.to_string());
        None
    }
    fn symlink(&mut self, target: &str, link: &str) -> Option<String> {
        self.note(format!("symlink {target} → {link}"));
        self.files.insert(link.to_string());
        None
    }
    fn run(&mut self, argv: &[String], _timeout: f64) -> Option<RunOut> {
        let line = argv.join(" ");
        self.note(format!("run {line}"));
        let mut res = RunOut {
            rc: 1,
            stdout: String::new(),
            stderr: String::new(),
        };
        for (needle, rc, out, err) in self.runs.clone() {
            if line.contains(&needle) {
                res = RunOut {
                    rc,
                    stdout: out,
                    stderr: err,
                };
                break;
            }
        }
        // A successful ffmpeg leaves the file as the last argv argument —
        // the flow then checks that it exists.
        if res.rc == 0 {
            if let Some(last) = argv.last() {
                if last.starts_with('/') {
                    self.files.insert(last.clone());
                }
            }
        }
        Some(res)
    }
    fn spawn(&mut self, argv: &[String], detached: bool) -> Result<i64, String> {
        self.note(format!("spawn[{}] {}", detached, argv.join(" ")));
        match self.spawn_err.clone() {
            Some(e) => Err(e),
            None => Ok(1),
        }
    }
    fn poll(&mut self, _proc: i64) -> Option<i64> {
        self.polls += 1;
        match self.exit_after_polls {
            Some(n) if self.polls > n => Some(self.exit_code),
            _ => None,
        }
    }
    fn wait(&mut self, _proc: i64, timeout: f64) -> Option<i64> {
        self.note(format!("wait {timeout}"));
        self.now += timeout;
        Some(self.exit_code)
    }
    fn terminate(&mut self, _proc: i64) {
        self.note("terminate".into());
    }
    fn kill(&mut self, _proc: i64) {
        self.note("kill".into());
    }
    fn monotonic(&mut self) -> f64 {
        self.now
    }
    fn sleep(&mut self, seconds: f64) {
        self.now += seconds;
    }
    fn choice(&mut self, items: &[String]) -> Option<String> {
        items.first().cloned()
    }
    fn ipc_send(&mut self, _sock: &str, payload: &str) -> bool {
        self.note(format!("ipc {}", payload.trim()));
        true
    }
    fn ipc_request(&mut self, _sock: &str, _payload: &str, _marker: &str) -> Option<String> {
        self.ipc_reply.clone()
    }
    fn notify(&mut self, addr: &str, message: &str) {
        self.note(format!("notify {addr} {message}"));
    }
    fn acquire_lock(&mut self, path: &str) -> bool {
        self.note(format!("lock {path}"));
        self.lock_free
    }
    fn kwin_reconfigure_dbus(&mut self, _timeout: f64) -> bool {
        self.note("kwin_reconfigure".into());
        true
    }
    fn x11_create(&mut self) -> Option<i64> {
        self.note("x11_create".into());
        if self.overlay {
            Some(5)
        } else {
            None
        }
    }
    fn x11_window_id(&mut self, _overlay: i64) -> i64 {
        0x0540_0007
    }
    fn x11_set_poster(&mut self, _overlay: i64, png: &str) -> bool {
        self.note(format!("x11_poster {png}"));
        true
    }
    fn x11_map(&mut self, _overlay: i64) {
        self.note("x11_map".into());
    }
    fn x11_compositor_present(&mut self, _overlay: i64) -> bool {
        self.note("x11_compositor?".into());
        self.compositor
    }
    fn x11_raise_start(&mut self, _overlay: i64) {
        self.note("x11_raise_start".into());
    }
    fn x11_raise_stop(&mut self, _overlay: i64) {
        self.note("x11_raise_stop".into());
    }
    fn x11_close(&mut self, _overlay: i64) {
        self.note("x11_close".into());
    }
    fn step_install_hybrid(&mut self, video: &str) -> (bool, String) {
        self.note(format!("step hyb {video}"));
        self.steps
            .get("hyb")
            .cloned()
            .unwrap_or((true, "ok".into()))
    }
    fn step_uninstall_hybrid(&mut self) -> (bool, String) {
        self.note("step unhyb".into());
        (true, "ok".into())
    }
    fn step_install_desktop_autostart(&mut self) -> (bool, String) {
        self.note("step desk".into());
        (true, "Заставка при входе включена".into())
    }
    fn step_remove_desktop_autostart(&mut self) {
        self.note("step rmdesk".into());
    }
    fn step_hybrid_supported(&mut self) -> bool {
        self.steps.contains_key("supported")
    }
    fn log(&mut self, level: &str, message: &str) {
        self.journal.push(format!("log[{level}] {message}"));
    }
}

/// A video from the fake Steam tree (shared HOME/VIDEO — above).
const FLOW_VIDEO: &str = "/HOME/.local/share/Steam/steamui/movies/deck_startup.webm";

/// A "video chosen" config is already in place.
fn with_config(io: FakeIo, extra: &str) -> FakeIo {
    let mut io = io;
    let path = "/HOME/.config/rexon/session_splash.json";
    io.texts.insert(
        path.to_string(),
        format!("{{\"path\": \"{FLOW_VIDEO}\", \"sound\": true{extra}}}"),
    );
    io.files.insert(path.to_string());
    io
}

#[test]
fn flow_play_no_file_stops_before_any_effect() {
    let _lang = i18n_pin::en();
    let mut io = FakeIo::new();
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(!ok);
    assert_eq!(msg, ss::msg_no_file());
    // neither a lock nor processes: a refusal BEFORE the first side action
    assert!(io.journal.iter().all(|l| !l.starts_with("spawn")), "{:?}", io.journal);
    assert!(!io.has("lock "));
}

#[test]
fn flow_play_second_run_skipped_by_lock() {
    let _lang = i18n_pin::en();
    let mut io = with_config(FakeIo::new().with_file(FLOW_VIDEO).with_bin("mpv"), "");
    io.lock_free = false;
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    // "already playing" is a success: the second instance leaves silently
    assert!(ok);
    assert_eq!(msg, ss::msg_already_playing());
    assert!(io.journal.iter().all(|l| !l.starts_with("spawn")));
}

#[test]
fn flow_play_wayland_helper_relay_order() {
    let _lang = i18n_pin::en();
    // Hybrid on Wayland: the helper came up, ksplashqml went out, mpv was unpaused.
    let helper = "/repo/rust/target/release/rexon-splash-overlay";
    let sock = "/run/user/1000/rexon-splash-mpv.sock";
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_file(helper)
            .with_file(sock)
            .with_bin("mpv")
            .with_bin("pgrep")
            .with_env("WAYLAND_DISPLAY", "wayland-0")
            .with_env("XDG_RUNTIME_DIR", "/run/user/1000")
            .with_env("NOTIFY_SOCKET", "/run/systemd/notify")
            .with_run("pgrep", 1, "", ""), // ksplashqml is not running
        ", \"hybrid_installed\": true",
    );
    io.ipc_reply = Some("{\"request_id\":700,\"error\":\"success\",\"data\":true}\n".into());
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    assert_eq!(msg, ss::msg_played());
    // Order: lock → helper start → READY=1 → unpause → quit.
    let i_lock = io.index_of("lock ");
    let i_spawn = io.index_of("spawn[false] /repo/rust/target/release/rexon-splash-overlay");
    let i_ready = io.index_of("notify /run/systemd/notify READY=1");
    let i_unpause = io.index_of("ipc {\"command\": [\"set_property\", \"pause\", false]}");
    let i_quit = io.index_of("ipc {\"command\": [\"quit\"]}");
    assert!(i_lock < i_spawn && i_spawn < i_ready && i_ready < i_unpause && i_unpause < i_quit,
            "{:#?}", io.journal);
    // Windowed mpv was not started — the helper managed by itself.
    assert!(!io.has("spawn[false] mpv"), "{:#?}", io.journal);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_helper_dead_falls_back_to_window_mpv() {
    let _lang = i18n_pin::ru();
    // The helper died before the IPC socket → a transparent fallback to windowed mpv + the KWin rule.
    let helper = "/repo/rust/target/release/rexon-splash-overlay";
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_file(helper)
            .with_bin("mpv")
            .with_env("WAYLAND_DISPLAY", "wayland-0")
            .with_env("XDG_RUNTIME_DIR", "/run/user/1000"),
        "",
    );
    io.exit_after_polls = Some(0); // the helper is dead on the very first probe
    let (ok, _msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok);
    assert!(io.has(ss::MSG_HELPER_NO_LIFTOFF), "{:#?}", io.journal);
    let cmd = io
        .journal
        .iter()
        .find(|l| l.starts_with("spawn[false] mpv"))
        .expect("оконный mpv не запущен");
    assert!(cmd.contains("--wayland-app-id=rexon-session-splash"), "{cmd}");
    // the KWin rule is set BEFORE the window starts
    assert!(io.index_of("write /HOME/.config/kwinrulesrc") < io.index_of("spawn[false] mpv"));
}

#[test]
fn flow_play_x11_overlay_poster_before_map() {
    let _lang = i18n_pin::en();
    // X11: the poster goes into the layer BEFORE mapping, no hybrid relay needed.
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_file("/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash/contents/splash/images/poster.png")
            .with_bin("mpv")
            .with_env("DISPLAY", ":0")
            .with_env("XDG_RUNTIME_DIR", "/run/user/1000")
            .with_env("NOTIFY_SOCKET", "@rexon")
            ,
        ", \"hybrid_installed\": true",
    );
    io.overlay = true;
    io.compositor = true;
    io.ipc_reply = Some("{\"request_id\":700,\"error\":\"success\",\"data\":true}\n".into());
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    let i_poster = io.index_of("x11_poster");
    let i_map = io.index_of("x11_map");
    let i_spawn = io.index_of("spawn[false] mpv");
    assert!(i_poster < i_map && i_map < i_spawn, "{:#?}", io.journal);
    // the mpv window draws into the layer
    assert!(io.has("--wid=88080391"), "{:#?}", io.journal);
    // on X11 the hybrid is forcibly off: no pause (and no unpause)
    assert!(!io.has("--pause"), "{:#?}", io.journal);
    assert!(!io.has("set_property"), "{:#?}", io.journal);
    // finale: first the layer goes out, then mpv
    assert!(io.index_of("x11_close") < io.index_of("ipc {\"command\": [\"quit\"]}"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_x11_cold_start_waits_for_compositor() {
    let _lang = i18n_pin::ru();
    // A cold login: there is no compositor yet — we wait for it under a black layer.
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_bin("mpv")
            .with_bin("ffmpeg")
            .with_env("DISPLAY", ":0")
            .with_env("XDG_RUNTIME_DIR", "/run/user/1000"),
        "",
    );
    io.overlay = true;
    io.compositor = false;
    io.ipc_reply = Some("{\"request_id\":700,\"error\":\"success\",\"data\":true}\n".into());
    let t0 = io.now;
    let (ok, _msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok);
    let probes = io.steps_of("x11_compositor?").len();
    assert!(probes > 1, "композитор не опрашивался повторно: {probes}");
    assert!(io.now - t0 >= ss::COMPOSITOR_WAIT_SEC, "ожидание короче потолка");
}

/// The path of the user's ksplashrc in the fake HOME.
const KSPLASHRC: &str = "/HOME/.config/ksplashrc";
/// The hybrid's LnF theme directory in the fake HOME.
const THEME_DIR: &str = "/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash";

/// Fixture "the hybrid is installed, a real login is in progress" (Wayland + helper).
fn heal_fixture(ksplashrc: Option<&str>) -> FakeIo {
    let helper = "/repo/rust/target/release/rexon-splash-overlay";
    let sock = "/run/user/1000/rexon-splash-mpv.sock";
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_file(helper)
            .with_file(sock)
            .with_dir(THEME_DIR)
            .with_bin("mpv")
            .with_bin("pgrep")
            .with_bin("kwriteconfig6")
            .with_env("WAYLAND_DISPLAY", "wayland-0")
            .with_env("XDG_RUNTIME_DIR", "/run/user/1000")
            .with_run("pgrep", 1, "", "")
            .with_run("kwriteconfig6", 0, "", ""),
        ", \"hybrid_installed\": true",
    );
    if let Some(text) = ksplashrc {
        io.files.insert(KSPLASHRC.to_string());
        io.texts.insert(KSPLASHRC.to_string(), text.to_string());
    }
    io.ipc_reply = Some("{\"request_id\":700,\"error\":\"success\",\"data\":true}\n".into());
    io
}

/// The self-healing decision: heal only when the hybrid is listed as installed and
/// ksplashrc holds a theme that is NOT ours (empty = the key was lost).
#[test]
fn heal_ksplashrc_decision() {
    let on = json!({"hybrid_installed": true});
    let off = json!({"hybrid_installed": false});
    let empty = json!({});
    // hybrid installed, the theme is someone else's or the key is lost → repair
    assert!(ss::should_heal_ksplashrc(&on, "org.kde.breezedark.desktop"));
    assert!(ss::should_heal_ksplashrc(&on, ""));
    // hybrid installed, the theme is already ours → leave it
    assert!(!ss::should_heal_ksplashrc(&on, ss::KSPLASH_THEME_ID));
    // no hybrid → someone else's ksplashrc is none of our business, whatever the value
    assert!(!ss::should_heal_ksplashrc(&off, "org.kde.breezedark.desktop"));
    assert!(!ss::should_heal_ksplashrc(&off, ""));
    assert!(!ss::should_heal_ksplashrc(&empty, "org.kde.breezedark.desktop"));
    assert!(!ss::should_heal_ksplashrc(&empty, ss::KSPLASH_THEME_ID));
}

/// Contention for the KSplash slot: the login video splash and the KDE splash share one place, and
/// the customer must choose. We check ALL branches of the decision: silence in either
/// direction is exactly the defect here.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn splash_slot_conflict_branches() {
    let _lang = i18n_pin::ru();
    // No hybrid — the slot is free, nothing to ask about, whatever the kind.
    assert!(ss::splash_slot_conflict("splash", "Breeze", false).is_none());
    assert!(ss::splash_slot_conflict("global", "Breeze", false).is_none());
    // The appearance kind does not concern the slot — also applied silently.
    for kind in ["colors", "cursor", "icons", "gtk", "lockscreen", "wallpapers", "none"] {
        assert!(!ss::apply_kind_takes_ksplash_slot(kind), "{kind}");
        assert!(ss::splash_slot_conflict(kind, "Breeze", true).is_none(), "{kind}");
    }
    // Our own poster theme: there is nobody to take the place from.
    assert!(ss::splash_slot_conflict("splash", ss::KSPLASH_THEME_ID, true).is_none());
    assert!(ss::splash_slot_conflict("global", ss::KSPLASH_THEME_ID, true).is_none());

    // Someone else's KDE splash over the hybrid — a question, and «No» means "do not
    // apply": for the `splash` kind the whole action is the splash.
    assert!(ss::apply_kind_takes_ksplash_slot("splash"));
    let c = ss::splash_slot_conflict("splash", "Breeze", true).expect("вопрос обязан быть");
    assert_eq!(c.title, ss::slot_question_title());
    assert!(!c.keep_applies_rest);
    assert!(c.question.contains("Breeze"), "{}", c.question);
    // Both outcomes are named in the text in plain words, otherwise «Yes»/«No» is a lottery.
    assert!(c.question.contains("«Да»") && c.question.contains("«Нет»"), "{}", c.question);
    assert_eq!(c.kept, ss::msg_slot_kept_instead_of("Breeze"));
    assert!(c.kept.contains("не применена"), "{}", c.kept);

    // A whole appearance: the splash is only one of its parts, so «No»
    // applies THE REST, and the slot stays with the video.
    assert!(ss::apply_kind_takes_ksplash_slot("global"));
    let g = ss::splash_slot_conflict("global", "Nordic", true).expect("вопрос обязан быть");
    assert_eq!(g.title, ss::slot_question_title());
    assert!(g.keep_applies_rest);
    assert!(g.question.contains("Nordic"), "{}", g.question);
    assert!(g.question.contains("«Да»") && g.question.contains("«Нет»"), "{}", g.question);
    assert_eq!(g.kept, ss::msg_slot_kept_video());
}

/// The texts of both outcomes are human and jargon-free: technology names
/// in Latin letters, "KSplash hybrid" is not shown to the customer.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn splash_slot_texts_are_human() {
    let _lang = i18n_pin::ru();
    let c = ss::splash_slot_conflict("splash", "Breeze", true).unwrap();
    let g = ss::splash_slot_conflict("global", "Nordic", true).unwrap();
    let released = ss::msg_slot_released();
    let failed = ss::msg_slot_release_failed("юнит не снялся");
    for text in [&c.question, &c.kept, &g.question, &g.kept.to_string(), &released, &failed] {
        assert!(!text.contains("гибрид"), "{text}");
        assert!(!text.contains("ksplashrc"), "{text}");
        assert!(!text.contains("КДЕ") && !text.contains("Плазма"), "{text}");
    }
    // A refusal must explain WHY the splash was not applied, not just
    // say "did not work".
    assert!(failed.contains("не применена"), "{failed}");
    assert!(released.contains("выключена"), "{released}");
}

/// ⚠️ 09.09.2026: applying a global Plasma theme overwrote
/// `[KSplash] Theme` — at login the STOCK Plasma splash hung for a second, and
/// only then the video jumped out. The real run must restore the binding
/// ITSELF, before showing, and move the foreign theme into a backup for an honest rollback.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_heals_foreign_ksplashrc_theme() {
    let _lang = i18n_pin::ru();
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\nEngine=KSplashQML\n"));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    // the binding is restored with our theme and our engine
    let i_theme = io.index_of("--key Theme org.rexon.sessionsplash");
    let i_engine = io.index_of("--key Engine KSplashQML");
    // …and necessarily BEFORE the player starts
    let i_spawn = io.index_of("spawn[false] /repo/rust/target/release/rexon-splash-overlay");
    assert!(i_theme < i_spawn && i_engine < i_spawn, "{:#?}", io.journal);
    // the foreign theme went into the backup: the hybrid rollback returns IT to the customer, not ours
    let cfg: Value = serde_json::from_str(
        io.written
            .get("/HOME/.config/rexon/session_splash.json")
            .expect("конфиг с бэкапом не записан"),
    )
    .unwrap();
    assert_eq!(cfg["prev_ksplash_theme"], json!("org.kde.breezedark.desktop"));
    assert_eq!(cfg["prev_ksplash_engine"], json!("KSplashQML"));
    // the theme directory was in place — no regeneration was started
    assert!(!io.has("-frames:v"), "{:#?}", io.journal);
}

/// The key was lost entirely (a live case: ksplashrc turned out EMPTY) — then an
/// empty string goes into the backup, and rollback restores the "there was no key" state.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_heals_empty_ksplashrc() {
    let _lang = i18n_pin::ru();
    let mut io = heal_fixture(Some(""));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    assert!(io.has("--key Theme org.rexon.sessionsplash"), "{:#?}", io.journal);
    let cfg: Value = serde_json::from_str(
        io.written
            .get("/HOME/.config/rexon/session_splash.json")
            .expect("конфиг с бэкапом не записан"),
    )
    .unwrap();
    assert_eq!(cfg["prev_ksplash_theme"], json!(""));
}

/// The binding is intact — ksplashrc is not touched and the config is not rewritten.
#[test]
fn flow_play_healthy_ksplashrc_untouched() {
    let _lang = i18n_pin::en();
    let mut io = heal_fixture(Some(
        "[KSplash]\nTheme=org.rexon.sessionsplash\nEngine=KSplashQML\n",
    ));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    assert!(!io.has("kwriteconfig6"), "{:#?}", io.journal);
    assert!(!io.written.contains_key("/HOME/.config/rexon/session_splash.json"));
}

/// The poster theme directory disappeared (`~/.local/share` was wiped) — we regenerate
/// it from the ACTUAL video path, the login does not stay black.
#[test]
fn flow_play_rebuilds_missing_theme_dir() {
    let _lang = i18n_pin::en();
    let mut io = heal_fixture(Some(
        "[KSplash]\nTheme=org.rexon.sessionsplash\nEngine=KSplashQML\n",
    ));
    io.dirs.remove(THEME_DIR);
    io.bins.insert("ffmpeg".into(), "/usr/bin/ffmpeg".into());
    io.runs.push(("-frames:v".into(), 0, String::new(), String::new()));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    let poster = format!("{THEME_DIR}/contents/splash/images/poster.png");
    assert!(io.has(&format!("-i {FLOW_VIDEO}")), "{:#?}", io.journal);
    assert!(io.has(&poster), "{:#?}", io.journal);
    assert!(io.has(ss::MSG_HEAL_THEME_REBUILT), "{:#?}", io.journal);
}

/// Self-healing does not break the show: kwriteconfig6 is missing — we complain into
/// the log, but play the video anyway.
#[test]
fn flow_play_heal_failure_does_not_stop_playback() {
    let _lang = i18n_pin::en();
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\n"));
    io.bins.remove("kwriteconfig6");
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    assert_eq!(msg, ss::msg_played());
    assert!(io.has(ss::MSG_HEAL_KSPLASHRC_FAILED), "{:#?}", io.journal);
}

/// A splash test from the UI (non-blocking) — someone else's ksplashrc is none of our business:
/// the user did not log in, the login binding is irrelevant.
#[test]
fn flow_play_nonblocking_does_not_touch_ksplashrc() {
    let _lang = i18n_pin::en();
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\n"));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, false, None, "/repo");
    assert!(ok, "{msg}");
    assert!(!io.has("kwriteconfig6"), "{:#?}", io.journal);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_install_hybrid_writes_theme_unit_and_config() {
    let _lang = i18n_pin::ru();
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_bin("ffmpeg")
            .with_bin("kwriteconfig6")
            .with_bin("systemctl")
            .with_bin("rexon")
            .with_run("-frames:v", 0, "", "")
            .with_run("kwriteconfig6", 0, "", "")
            .with_run("systemctl", 0, "", ""),
        "",
    );
    let (ok, msg) = Splash::new(&mut io, HOME).install_hybrid(FLOW_VIDEO, "/usr/bin/python3");
    assert!(ok, "{msg}");
    assert_eq!(msg, ss::msg_hybrid_installed());
    let theme = "/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash";
    assert!(io.written.contains_key(&format!("{theme}/metadata.json")));
    assert!(io.written.contains_key(&format!("{theme}/contents/splash/Splash.qml")));
    let unit = "/HOME/.config/systemd/user/rexon-session-splash.service";
    let text = io.written.get(unit).expect("юнит не записан");
    // ⚠️ the threshold that caused the panel over the splash on a cold login
    assert!(text.contains("TimeoutStartSec=15"), "{text}");
    assert!(text.contains("Type=notify") && text.contains("Before=plasma-plasmashell.service"));
    // ksplashrc is edited AFTER backing up the previous theme into the config
    let cfg = io
        .written
        .get("/HOME/.config/rexon/session_splash.json")
        .expect("конфиг не записан");
    assert!(cfg.contains("hybrid_installed"), "{cfg}");
    assert!(io.index_of("write_atomic /HOME/.config/rexon/session_splash.json")
        < io.index_of("run /usr/bin/kwriteconfig6"));
    assert!(io.has("run /usr/bin/systemctl --user enable rexon-session-splash.service"));
}

#[test]
fn flow_install_hybrid_without_kwriteconfig_stops_with_reason() {
    let _lang = i18n_pin::en();
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_bin("ffmpeg")
            // There IS something to start the splash with — we check precisely the ksplashrc refusal,
            // not a missing entry (that has its own watchdog below).
            .with_bin("rexon")
            .with_run("-frames:v", 0, "", ""),
        "",
    );
    let (ok, msg) = Splash::new(&mut io, HOME).install_hybrid(FLOW_VIDEO, "/usr/bin/python3");
    assert!(!ok);
    assert_eq!(msg, ss::msg_ksplashrc_failed());
    // the unit is not written if ksplashrc did not give in — there is no such thing as half a hybrid
    assert!(!io.has("systemd/user/rexon-session-splash.service"), "{:#?}", io.journal);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_uninstall_hybrid_restores_previous_theme() {
    let _lang = i18n_pin::ru();
    let unit = "/HOME/.config/systemd/user/rexon-session-splash.service";
    let theme = "/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash";
    let mut io = with_config(
        FakeIo::new()
            .with_file(unit)
            .with_dir(theme)
            .with_bin("kwriteconfig6")
            .with_bin("systemctl")
            .with_run("kwriteconfig6", 0, "", "")
            .with_run("systemctl", 0, "", ""),
        ", \"hybrid_installed\": true, \"prev_ksplash_theme\": \"org.kde.breeze.desktop\", \"prev_ksplash_engine\": \"KSplashQML\"",
    );
    io.texts.insert(
        "/HOME/.config/ksplashrc".into(),
        format!("[KSplash]\nTheme={}\n", ss::KSPLASH_THEME_ID),
    );
    let (ok, msg) = Splash::new(&mut io, HOME).uninstall_hybrid();
    assert!(ok, "{msg}");
    assert_eq!(msg, ss::msg_hybrid_uninstalled());
    assert!(io.has("run /usr/bin/kwriteconfig6"));
    assert!(io.has("--group KSplash --key Theme org.kde.breeze.desktop"), "{:#?}", io.journal);
    assert!(io.has(&format!("rmtree {theme}")));
    assert!(io.has(&format!("rm {unit}")));
    let cfg = io
        .written
        .get("/HOME/.config/rexon/session_splash.json")
        .expect("конфиг не записан");
    assert!(!cfg.contains("hybrid_installed") && !cfg.contains("prev_ksplash"), "{cfg}");
}

/// Login hooks follow the product's chosen entry: with the Rust entry both the hybrid unit
/// and the autostart `.desktop` call the window binary — without an interpreter, even when
/// the `rexon` launcher is in PATH. With the Python entry — the old lines.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_login_hooks_follow_launch_entry() {
    let _lang = i18n_pin::ru();
    use rexon_core::installer::LaunchEntry;
    const WIN: &str = "/opt/Rexon tree/rexon-main-window";
    let run = |entry: Option<LaunchEntry>| -> (String, String) {
        let mut io = with_config(
            FakeIo::new()
                .with_file(FLOW_VIDEO)
                .with_bin("ffmpeg")
                .with_bin("kwriteconfig6")
                .with_bin("systemctl")
                .with_bin("rexon")
                .with_run("-frames:v", 0, "", "")
                .with_run("kwriteconfig6", 0, "", "")
                .with_run("systemctl", 0, "", ""),
            "",
        );
        io.entry = entry.clone();
        let (ok, msg) = Splash::new(&mut io, HOME).install_hybrid(FLOW_VIDEO, "/usr/bin/python3");
        assert!(ok, "{msg}");
        let unit = io
            .written
            .get("/HOME/.config/systemd/user/rexon-session-splash.service")
            .expect("юнит не записан")
            .clone();
        let mut io = FakeIo::new().with_bin("rexon");
        io.entry = entry;
        let (ok, msg) = Splash::new(&mut io, HOME).install_desktop_autostart("/usr/bin/python3");
        assert!(ok, "{msg}");
        let desk = io
            .written
            .iter()
            .find(|(p, _)| p.contains("/autostart/"))
            .map(|(_, t)| t.clone())
            .expect("autostart не записан");
        (unit, desk)
    };

    let (unit, desk) = run(Some(LaunchEntry::MainWindow(WIN.into())));
    assert!(unit.contains(&format!("\nExecStart=\"{WIN}\" --session-splash\n")), "{unit}");
    assert!(desk.contains(&format!("\nExec=\"{WIN}\" --session-splash\n")), "{desk}");
    for text in [&unit, &desk] {
        for mark in ["python", "-m rexon", "/usr/bin/rexon"] {
            assert!(!text.contains(mark), "«{mark}» при Rust-входе:\n{text}");
        }
    }

    let (unit, desk) = run(None);
    assert!(unit.contains("\nExecStart=/usr/bin/rexon --session-splash\n"), "{unit}");
    assert!(desk.contains("\nExec=/usr/bin/rexon --session-splash\n"), "{desk}");
}

/// ⛔ There is nothing to start the splash with — a refusal OUT LOUD, not a write into the void.
///
/// Before 16.09.2026 (block H) at this fork — the Python entry and no `rexon` launcher
/// in `PATH` — both records got `python3 -m rexon.app
/// --session-splash` and reported success. Neither the `.desktop` autostart nor the
/// systemd unit provide a working directory, and the system interpreter's `sys.path`
/// has no `rexon` package: nothing was shown at login, and
/// there was no way to find out.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_login_hooks_refuse_when_nothing_to_launch() {
    let _lang = i18n_pin::ru();
    // There is NO `rexon` launcher in PATH (`with_bin("rexon")` not called).
    let mut io = with_config(
        FakeIo::new()
            .with_file(FLOW_VIDEO)
            .with_bin("ffmpeg")
            .with_bin("kwriteconfig6")
            .with_bin("systemctl")
            .with_run("-frames:v", 0, "", "")
            .with_run("kwriteconfig6", 0, "", "")
            .with_run("systemctl", 0, "", ""),
        "",
    );
    let (ok, msg) = Splash::new(&mut io, HOME).install_hybrid(FLOW_VIDEO, "/usr/bin/python3");
    assert!(!ok, "гибрид встал без единого способа запуститься");
    assert_eq!(msg, rexon_core::app::MSG_NOTHING_TO_LAUNCH);
    assert!(
        !io.written.keys().any(|p| p.contains("rexon-session-splash.service")),
        "юнит записан мёртвой строкой: {:?}",
        io.written.keys().collect::<Vec<_>>()
    );

    let mut io = FakeIo::new();
    let (ok, msg) = Splash::new(&mut io, HOME).install_desktop_autostart("/usr/bin/python3");
    assert!(!ok, "автозапуск встал без единого способа запуститься");
    assert_eq!(msg, rexon_core::app::MSG_NOTHING_TO_LAUNCH);
    assert!(
        !io.written.keys().any(|p| p.contains("/autostart/")),
        "запись автозапуска создана мёртвой: {:?}",
        io.written.keys().collect::<Vec<_>>()
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_install_autostart_routes_through_steps() {
    let _lang = i18n_pin::ru();
    // Hybrid possible and installed → the .desktop is removed.
    let mut io = with_config(FakeIo::new(), "");
    io.steps.insert("supported".into(), (true, String::new()));
    let (ok, msg) = Splash::new(&mut io, HOME).install_autostart();
    assert!(ok && msg == ss::msg_hybrid_route_ok(), "{msg}");
    assert_eq!(io.steps_of("step "), vec![format!("step hyb {FLOW_VIDEO}"), "step rmdesk".to_string()]);

    // Hybrid failed → a full rollback of the leftovers and an honest phase 1.
    let mut io = with_config(FakeIo::new(), "");
    io.steps.insert("supported".into(), (true, String::new()));
    io.steps.insert("hyb".into(), (false, "нет kwriteconfig6".into()));
    let (ok, msg) = Splash::new(&mut io, HOME).install_autostart();
    assert!(ok, "{msg}");
    assert!(msg.starts_with("Гибрид не удался"), "{msg}");
    assert_eq!(
        io.steps_of("step "),
        vec![format!("step hyb {FLOW_VIDEO}"), "step unhyb".to_string(), "step desk".to_string()]
    );

    // No video chosen — not a single step.
    let mut io = FakeIo::new();
    let (ok, msg) = Splash::new(&mut io, HOME).install_autostart();
    assert!(!ok && msg == ss::msg_choose_first());
    assert!(io.steps_of("step ").is_empty());
}

#[test]
fn flow_hybrid_supported_short_circuits() {
    let _lang = i18n_pin::en();
    // Not KDE — the binaries are not even looked for.
    let mut io = FakeIo::new().with_env("XDG_CURRENT_DESKTOP", "GNOME");
    assert!(!Splash::new(&mut io, HOME).hybrid_supported());
    assert!(io.journal.is_empty(), "{:#?}", io.journal);

    // KDE, but no ffmpeg — systemctl is not touched.
    let mut io = FakeIo::new().with_env("XDG_CURRENT_DESKTOP", "KDE");
    assert!(!Splash::new(&mut io, HOME).hybrid_supported());
    assert!(!io.has("run "), "{:#?}", io.journal);

    // Everything in place and the target active.
    let mut io = FakeIo::new()
        .with_env("XDG_CURRENT_DESKTOP", "KDE")
        .with_bin("ffmpeg")
        .with_bin("kwriteconfig6")
        .with_bin("kreadconfig6")
        .with_bin("ksplashqml")
        .with_bin("systemctl")
        .with_run("is-active", 0, "active\n", "");
    assert!(Splash::new(&mut io, HOME).hybrid_supported());
    assert!(io.has("is-active plasma-workspace.target"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_scan_reads_steam_tree() {
    let _lang = i18n_pin::ru();
    let root = "/HOME/.local/share/Steam";
    let mut io = FakeIo::new()
        .with_dir(root)
        .with_dir(&format!("{root}/config"))
        .with_dir(&format!("{root}/steamui/movies"))
        .with_file(&format!("{root}/steamui/movies/deck_startup.webm"))
        .with_dir(&format!("{root}/config/communityitemscache/startupmovies"))
        .with_file(&format!("{root}/config/communityitemscache/startupmovies/34806_ab.webm"))
        .with_dir(&format!("{root}/userdata"))
        .with_dir(&format!("{root}/userdata/123"))
        .with_file(&format!("{root}/userdata/123/config/localconfig.vdf"));
    io.texts.insert(
        format!("{root}/userdata/123/config/localconfig.vdf"),
        "\"UserLocalConfigStore\"\n{\n\"GetStartupMovies\"\t\t\"[{\\\"communityitemid\\\": \\\"34806\\\", \\\"name\\\": \\\"Special Delivery\\\"}]\"\n}\n".into(),
    );
    let movies = Splash::new(&mut io, HOME).scan_startup_movies();
    let ids: Vec<&str> = movies.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["deck_startup", "34806"]);
    assert_eq!(movies[0]["name"], "Steam Deck (стандартная)");
    assert_eq!(movies[1]["name"], "Special Delivery");
    assert_eq!(movies[1]["origin"], "shop");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_preview_frame_retries_first_frame_and_caches() {
    let _lang = i18n_pin::ru();
    let video = "/видео.webm";
    let cache = "/HOME/.cache/rexon/steam_splash_preview";
    let mut io = FakeIo::new()
        .with_file(video)
        .with_bin("ffmpeg")
        .with_bin("ffprobe")
        .with_run("ffprobe", 0, "12.5\n", "")
        // the first attempt (with an offset) failed, the second one from the first frame
        .with_run("-ss", 1, "", "битый")
        .with_run("-frames:v", 0, "", "");
    let out = {
        let mut sp = Splash::new(&mut io, HOME);
        sp.preview_frame(video)
    };
    let out = out.expect("кадр не получен");
    assert!(out.starts_with(cache), "{out}");
    let runs = io.steps_of("run ");
    assert_eq!(runs.len(), 3, "{runs:#?}"); // ffprobe + two ffmpeg attempts
    assert!(runs[1].contains("-ss "), "{:?}", runs[1]);
    assert!(!runs[2].contains("-ss "), "{:?}", runs[2]);
    assert!(io.has(&format!("replace {out}.part.png → {out}")));

    // A repeat call takes the ready file: ffmpeg is no longer touched.
    io.journal.clear();
    let again = Splash::new(&mut io, HOME).preview_frame(video);
    assert_eq!(again.as_deref(), Some(out.as_str()));
    assert!(io.steps_of("run ").is_empty(), "{:#?}", io.journal);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_rotation_picks_next_and_saves() {
    let _lang = i18n_pin::ru();
    let a = "/a.webm";
    let b = "/b.webm";
    let mut io = FakeIo::new().with_file(a).with_file(b);
    io.texts.insert(
        "/HOME/.config/rexon/session_splash.json".into(),
        format!("{{\"path\": \"{a}\", \"rotate\": true, \"favorites\": [\"{a}\", \"{b}\"]}}"),
    );
    io.files
        .insert("/HOME/.config/rexon/session_splash.json".into());
    let nxt = Splash::new(&mut io, HOME).advance_rotation();
    // the current video is excluded from the draw — otherwise the "randomness" would stand still
    assert_eq!(nxt.as_deref(), Some(b));
    let cfg = io
        .written
        .get("/HOME/.config/rexon/session_splash.json")
        .expect("конфиг не записан");
    assert!(cfg.contains(b), "{cfg}");
}

#[test]
fn letterbox_rect_matches_mpv_fullscreen() {
    // A wide video on a 16:10 screen: bars at the top and bottom.
    assert_eq!(ss::letterbox_rect(1920, 1080, 2560, 1600), (0, 80, 2560, 1440));
    // A vertical video: bars on the left and right.
    assert_eq!(ss::letterbox_rect(1080, 1920, 2560, 1600), (830, 0, 900, 1600));
    // An exact aspect match — no bars.
    assert_eq!(ss::letterbox_rect(1280, 800, 2560, 1600), (0, 0, 2560, 1600));
    // A broken image (zero size) does not crash the layer — we fill it entirely.
    assert_eq!(ss::letterbox_rect(0, 0, 640, 480), (0, 0, 640, 480));
}

/// The hybrid is installed: the flag in the config, the unit and the theme in place, rollback works.
fn slot_fixture() -> FakeIo {
    let unit = "/HOME/.config/systemd/user/rexon-session-splash.service";
    let theme = "/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash";
    with_config(
        FakeIo::new()
            .with_file(unit)
            .with_dir(theme)
            .with_bin("kwriteconfig6")
            .with_bin("systemctl")
            .with_run("kwriteconfig6", 0, "", "")
            .with_run("systemctl", 0, "", ""),
        ", \"hybrid_installed\": true",
    )
}

/// The slot contention flow shared by BOTH owners of the «Appearance» tab
/// (the Rust window and the Python owner of the live product): the branching "Yes/No → what
/// to do" lives in the core, and every branch is checked here.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_resolve_splash_slot_all_answers() {
    let _lang = i18n_pin::ru();
    let unit_rm = "rm /HOME/.config/systemd/user/rexon-session-splash.service";

    // The kind does not concern the slot — no question, the disk is not read at all.
    let mut io = FakeIo::new();
    let plan = Splash::new(&mut io, HOME)
        .resolve_splash_slot("colors", "Breeze", |_| panic!("вопроса быть не должно"));
    assert_eq!(plan, ss::SplashSlotPlan::plain());
    assert!(io.journal.is_empty(), "{:#?}", io.journal);

    // No hybrid — no contention.
    let mut io = FakeIo::new();
    let plan = Splash::new(&mut io, HOME)
        .resolve_splash_slot("splash", "Breeze", |_| panic!("вопроса быть не должно"));
    assert_eq!(plan, ss::SplashSlotPlan::plain());

    // Splash, «No» — do not apply, do not touch the hybrid.
    let mut io = slot_fixture();
    let mut asked = None;
    let plan = Splash::new(&mut io, HOME).resolve_splash_slot("splash", "Breeze", |c| {
        asked = Some(c.title);
        false
    });
    assert_eq!(asked, Some(ss::slot_question_title()));
    assert_eq!(plan, ss::SplashSlotPlan::Stop(ss::msg_slot_kept_instead_of("Breeze")));
    assert!(!io.has(unit_rm), "{:#?}", io.journal);

    // A whole appearance, «No» — apply everything and give the slot back to the video.
    let mut io = slot_fixture();
    let plan = Splash::new(&mut io, HOME).resolve_splash_slot("global", "Nordic", |_| false);
    assert_eq!(
        plan,
        ss::SplashSlotPlan::Apply {
            keep_video: Some(FLOW_VIDEO.to_string()),
            note: ss::msg_slot_kept_video(),
        }
    );
    assert!(!io.has(unit_rm), "{:#?}", io.journal);

    // «Yes» — the hybrid is removed BEFORE applying, the place goes to the chosen splash.
    let mut io = slot_fixture();
    let plan = Splash::new(&mut io, HOME).resolve_splash_slot("splash", "Breeze", |_| true);
    assert_eq!(
        plan,
        ss::SplashSlotPlan::Apply { keep_video: None, note: ss::msg_slot_released() }
    );
    assert!(io.has(unit_rm), "{:#?}", io.journal);
}

/// Finishing the plan: the slot goes back to the video, the note is only added on success.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_finish_splash_slot_heals_and_notes() {
    let _lang = i18n_pin::ru();
    assert_eq!(ss::msg_with_slot_note("Готово.", "Ролик на месте.", true), "Готово. Ролик на месте.");
    assert_eq!(ss::msg_with_slot_note("", "Ролик на месте.", true), "Ролик на месте.");
    assert_eq!(ss::msg_with_slot_note("Сбой.", "Ролик на месте.", false), "Сбой.");

    // Without a video — nothing to heal.
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\n"));
    let out = Splash::new(&mut io, HOME).finish_splash_slot(None, "", true, "Готово.");
    assert_eq!(out, "Готово.");
    assert!(!io.has("kwriteconfig6"), "{:#?}", io.journal);

    // The video is kept — the binding is restored with our theme even on failure.
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\n"));
    let out = Splash::new(&mut io, HOME).finish_splash_slot(
        Some(FLOW_VIDEO),
        &ss::msg_slot_kept_video(),
        false,
        "Сбой.",
    );
    assert_eq!(out, "Сбой.");
    assert!(io.has("--key Theme org.rexon.sessionsplash"), "{:#?}", io.journal);
}

// ============================================================================
// English interface
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// All splash messages in English — without a single Russian letter.
#[test]
fn english_splash_texts_have_no_russian() {
    let _lang = i18n_pin::en();
    let e = "E";
    let mut all: Vec<String> = vec![
        ss::msg_no_file(),
        ss::msg_no_mpv(),
        ss::msg_already_playing(),
        ss::msg_played(),
        ss::msg_launched(),
        ss::msg_mpv_timeout(),
        ss::msg_helper_timeout(),
        ss::msg_choose_first(),
        ss::msg_autostart_enabled(),
        ss::msg_autostart_disabled(),
        ss::msg_hybrid_route_ok(),
        ss::msg_hybrid_installed(),
        ss::msg_hybrid_uninstalled(),
        ss::msg_lnf_updated(),
        ss::mpv_exit_message(3),
        ss::helper_exit_message(3),
        ss::hybrid_fallback_message(e, true, e),
        ss::msg_theme_dir_failed(e),
        ss::msg_theme_write_failed(e),
        ss::msg_no_ffmpeg(),
        ss::msg_ffmpeg_failed(e),
        ss::msg_ffmpeg_stderr(e),
        ss::msg_ksplashrc_failed(),
        // ksplashrc self-healing (`msg_heal_*`) writes only to the log — it is
        // not translated.
        ss::msg_unit_write_failed(e),
        ss::msg_unit_enable_failed(e),
        ss::msg_rollback_problems(e),
        ss::msg_problem_unit(e),
        ss::msg_problem_ksplashrc(),
        ss::msg_problem_theme(e),
        ss::msg_problem_autostart(e),
        ss::msg_autostart_write_failed(e),
        ss::msg_config_write_failed(e),
        ss::msg_mpv_spawn_failed(e),
        ss::msg_slot_kept_video(),
        ss::msg_slot_kept_instead_of("Breeze"),
        ss::msg_slot_released(),
        ss::msg_slot_release_failed(e),
        ss::slot_question_title().to_string(),
    ];
    for origin in [ss::ORIGIN_BUILTIN, ss::ORIGIN_SHOP, ss::ORIGIN_CUSTOM] {
        all.push(ss::origin_label(origin));
    }
    for (_, name) in ss::BUILTIN_NAMES {
        all.push(rexon_core::i18n::tr(name));
    }
    for kind in ["global", "splash"] {
        let c = ss::splash_slot_conflict(kind, "Breeze", true).expect("спор за слот");
        all.push(c.title.to_string());
        all.push(c.question);
        all.push(c.kept);
    }
    for text in &all {
        assert!(!text.is_empty());
        assert!(!has_cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// The English splash phrases read as phrases, not as a calque.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(ss::origin_label(ss::ORIGIN_BUILTIN), "Built into Steam");
    assert_eq!(ss::origin_label(ss::ORIGIN_CUSTOM), "Your own file");
    assert_eq!(ss::msg_choose_first(), "Choose a startup movie first");
    assert_eq!(ss::mpv_exit_message(2), "mpv exited with code 2");
    assert_eq!(
        ss::hybrid_fallback_message("ffmpeg isn't installed", true, ""),
        "The hybrid didn't work (ffmpeg isn't installed) — regular autostart is on instead"
    );
    assert_eq!(
        ss::msg_slot_kept_instead_of("Breeze"),
        "The “Breeze” splash screen wasn't applied — the Rexon startup movie stays at login."
    );
    assert_eq!(ss::slot_question_title(), "One Splash Screen at a Time");
    let c = ss::splash_slot_conflict("splash", "Breeze", true).unwrap();
    assert!(c.question.contains("“Yes” — apply the “Breeze” splash screen."), "{}", c.question);
}

/// The same texts in Russian — verbatim as before moving to the catalog (snapshot
/// `ca42e74^`), with «guillemets».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_slot_texts_as_before() {
    let _lang = i18n_pin::ru();
    assert_eq!(ss::origin_label(ss::ORIGIN_BUILTIN), "Встроенная в Steam");
    assert_eq!(ss::origin_label(ss::ORIGIN_CUSTOM), "Свой файл");
    assert_eq!(ss::origin_label(ss::ORIGIN_SHOP), "Points Shop");
    assert_eq!(ss::msg_choose_first(), "Сначала выберите заставку");
    assert_eq!(ss::slot_question_title(), "Заставка при входе — место одно");
    assert_eq!(
        ss::msg_slot_kept_instead_of("Breeze"),
        "Заставка «Breeze» не применена — при входе остаётся видеозаставка Rexon."
    );
    let c = ss::splash_slot_conflict("splash", "Breeze", true).unwrap();
    assert!(c.question.contains("«Да» — поставить заставку «Breeze»."), "{}", c.question);
    assert!(!c.question.contains('“') && !c.question.contains('”'), "{}", c.question);
}

/// The «None» tile in the «Splash Screen» section with a video enabled: the question is its own —
/// without the name "None" the person never saw; without a video there is no question.
#[test]
fn splash_none_tile_asks_in_own_words() {
    assert!(ss::splash_slot_conflict("splash", "None", false).is_none());
    let c = ss::splash_slot_conflict("splash", "None", true).expect("вопрос обязан быть");
    assert!(!c.question.contains("None"), "{}", c.question);
    assert!(!c.kept.contains("None"), "{}", c.kept);
    assert!(!c.keep_applies_rest, "«Нет» — значит не применять");
}

// ============================================================================
// Own video: the third scan source, file check, new built-in videos
// ============================================================================

/// Built-in videos — exactly those lying in the client's `steamui/movies`
/// (November 2026), in the old order and with Valve's new devices at the end.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn builtin_names_cover_new_steam_devices() {
    let keys: Vec<&str> = ss::BUILTIN_NAMES.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        vec![
            "deck_startup",
            "oled_startup",
            "bigpicture_startup",
            "steam_os_startup",
            "startup_machine",
            "steamframe_startup_450x450",
            "steam_os_family_startup",
        ]
    );
    let files = ss::builtin_movie_filenames();
    assert_eq!(files.len(), 7);
    assert!(files.contains(&"steamframe_startup_450x450.webm".to_string()));
    {
        let _lang = i18n_pin::en();
        let name = |k: &str| {
            let (_, n) = ss::BUILTIN_NAMES.iter().find(|(key, _)| *key == k).unwrap();
            rexon_core::i18n::tr(n)
        };
        assert_eq!(name("steam_os_startup"), "SteamOS");
        assert_eq!(name("startup_machine"), "Steam Machine");
        assert_eq!(name("steamframe_startup_450x450"), "Steam Frame");
        assert_eq!(name("steam_os_family_startup"), "Steam Deck (Factory Image)");
    }
    let _lang = i18n_pin::ru();
    let movies = ss::scan_movies(
        "/S",
        &["steam_os_family_startup.webm".to_string(), "startup_machine.webm".to_string()],
        &[],
        &json!({}),
        &[],
    );
    let names: Vec<&str> = movies.iter().map(|m| m["name"].as_str().unwrap()).collect();
    // The order follows the list, not the listing.
    assert_eq!(names, vec!["Steam Machine", "Steam Deck (заводской образ)"]);
    assert_eq!(movies[0]["path"], "/S/steamui/movies/startup_machine.webm");
}

/// `config/uioverrides/movies/*.webm` — as in Steam itself: only `.webm`
/// (case-sensitive), hidden ones are skipped, a `.jpg` thumbnail next to it.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn scan_reads_custom_movies_like_steam() {
    let _lang = i18n_pin::ru();
    let custom: Vec<String> = [
        "моя заставка.webm",
        "b.webm",
        "b.webm.jpg",
        "моя заставка.jpg",
        "a.webm",
        ".a.webm",
        ".webm",
        "clip.mp4",
        "LOUD.WEBM",
        "notes.txt",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    let out = ss::scan_movies(
        "/S",
        &["deck_startup.webm".to_string()],
        &["34806_ab.webm".to_string()],
        &json!({}),
        &custom,
    );
    let ids: Vec<&str> = out.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        vec![
            "deck_startup",
            "34806",
            "/S/config/uioverrides/movies/a.webm",
            "/S/config/uioverrides/movies/b.webm",
            "/S/config/uioverrides/movies/моя заставка.webm",
        ]
    );
    let mine = &out[4];
    assert_eq!(mine["name"], "моя заставка.webm"); // as in the Steam list
    assert_eq!(mine["title"], "моя заставка");
    assert_eq!(mine["builtin"], false);
    assert_eq!(mine["origin"], ss::ORIGIN_CUSTOM);
    assert_eq!(mine["origin_label"], "Свой ролик");
    assert_eq!(mine["thumb"], "/S/config/uioverrides/movies/моя заставка.jpg");
    assert_eq!(out[3]["thumb"], "/S/config/uioverrides/movies/b.webm.jpg");
    assert_eq!(out[2]["thumb"], "");
    // Enrichment does not turn our own video into "Points Shop".
    let enriched = ss::enrich_movies(&out, &json!({}), &[]);
    assert_eq!(enriched[4]["origin"], ss::ORIGIN_CUSTOM);
    assert_eq!(enriched[4]["origin_label"], "Свой ролик");
    assert_eq!(enriched[4]["title"], "моя заставка");
    assert_eq!(enriched[1]["origin"], ss::ORIGIN_SHOP);
    assert_eq!(enriched[0]["origin"], ss::ORIGIN_BUILTIN);
    // A manually chosen file (only Rexon sees it) is captioned as before.
    assert_eq!(ss::origin_label(ss::ORIGIN_CUSTOM), "Свой файл");
}

/// The live scan route: the executor lists `uioverrides/movies`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_scan_reads_custom_movies_dir() {
    let _lang = i18n_pin::ru();
    let root = "/HOME/.local/share/Steam";
    let dir = format!("{root}/config/uioverrides/movies");
    let mut io = FakeIo::new()
        .with_dir(root)
        .with_dir(&format!("{root}/config"))
        .with_dir(&format!("{root}/config/uioverrides"))
        .with_dir(&dir)
        .with_file(&format!("{dir}/intro.webm"))
        .with_file(&format!("{dir}/intro.jpg"));
    assert_eq!(ss::steam_custom_movies_dir(root), dir);
    let movies = Splash::new(&mut io, HOME).scan_startup_movies();
    assert_eq!(movies.len(), 1);
    assert_eq!(movies[0]["path"], format!("{dir}/intro.webm"));
    assert_eq!(movies[0]["thumb"], format!("{dir}/intro.jpg"));
    assert_eq!(movies[0]["origin"], "custom");
    assert_eq!(movies[0]["origin_label"], "Свой ролик");
    assert_eq!(movies[0]["title"], "intro");
}

/// A sample of `ffprobe -show_streams -show_format -of json` output.
fn probe_json(format: &str, duration: Option<&str>, streams: Value) -> String {
    let mut fmt = json!({"format_name": format});
    if let Some(d) = duration {
        fmt["duration"] = json!(d);
    }
    json!({"streams": streams, "format": fmt}).to_string()
}

fn vstream(codec: &str, w: i64, h: i64) -> Value {
    json!({"codec_type": "video", "codec_name": codec, "width": w, "height": h,
           "disposition": {"attached_pic": 0}})
}

fn astream(codec: &str) -> Value {
    json!({"codec_type": "audio", "codec_name": codec})
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn movie_probe_verdicts() {
    let _lang = i18n_pin::ru();
    use ss::MovieVerdict::{AsIs, Reject, Transcode};
    let webm = "matroska,webm";
    let mp4 = "mov,mp4,m4a,3gp,3g2,mj2";
    let check = |fmt: &str, d: Option<&str>, st: Value| ss::check_movie_probe(&probe_json(fmt, d, st));

    // As is: webm VP9/VP8/AV1 + Opus/Vorbis or without sound.
    let c = check(webm, Some("5.000000"), json!([vstream("vp9", 1920, 1200), astream("opus")]));
    assert_eq!(c.verdict, AsIs, "{c:?}");
    assert_eq!((c.width, c.height, c.duration), (1920, 1200, 5.0));
    assert_eq!((c.video_codec.as_str(), c.audio_codec.as_str()), ("vp9", "opus"));
    assert!(c.reason.is_empty() && c.warnings.is_empty());
    assert_eq!(check(webm, Some("3.1"), json!([vstream("vp8", 1280, 720), astream("vorbis")])).verdict, AsIs);
    assert_eq!(check(webm, Some("3.1"), json!([vstream("av1", 1280, 800)])).verdict, AsIs);
    // A vertical video from a phone — acceptable.
    assert_eq!(check(webm, Some("3"), json!([vstream("vp9", 720, 1280)])).verdict, AsIs);
    // Duration as a number rather than a string — also readable.
    let num = json!({"streams": [vstream("vp9", 1280, 720)],
                     "format": {"format_name": webm, "duration": 4.5}}).to_string();
    assert_eq!(ss::check_movie_probe(&num).duration, 4.5);

    // Re-encode: another container/codec, extra tracks, no duration.
    assert_eq!(check(mp4, Some("5"), json!([vstream("h264", 1920, 1080), astream("aac")])).verdict, Transcode);
    assert_eq!(check(webm, Some("5"), json!([vstream("h264", 1920, 1080)])).verdict, Transcode);
    assert_eq!(check(webm, Some("5"), json!([vstream("vp9", 1920, 1080), astream("aac")])).verdict, Transcode);
    assert_eq!(
        check(webm, Some("5"), json!([vstream("vp9", 1920, 1080), astream("opus"),
                                      {"codec_type": "subtitle", "codec_name": "ass"}])).verdict,
        Transcode
    );
    assert_eq!(
        check(webm, Some("5"), json!([vstream("vp9", 1920, 1080), astream("opus"), astream("opus")])).verdict,
        Transcode
    );
    let c = check(webm, None, json!([vstream("vp9", 1920, 1080)]));
    assert_eq!((c.verdict, c.duration), (Transcode, 0.0));

    // Refusals in human language.
    let c = ss::check_movie_probe("не json");
    assert_eq!(c.verdict, Reject);
    assert_eq!(c.reason, "Не удалось прочитать файл — возможно, он повреждён или это не видео.");
    assert_eq!(ss::check_movie_probe("{}").verdict, Reject);
    // An mp3 with album art: there is a "video stream", but no video.
    let cover = json!({"codec_type": "video", "codec_name": "mjpeg", "width": 1000, "height": 1000,
                       "disposition": {"attached_pic": 1}});
    let c = check("mp3", Some("200"), json!([astream("mp3"), cover]));
    assert_eq!(c.verdict, Reject);
    assert_eq!(c.reason, "В этом файле нет изображения — выберите видеофайл.");
    let c = check(webm, Some("0.5"), json!([vstream("vp9", 1920, 1080)]));
    assert_eq!(c.verdict, Reject);
    assert_eq!(c.reason, "Ролик короче секунды — он мелькнёт незаметно. Выберите ролик подлиннее.");
    let c = check(mp4, Some("60.5"), json!([vstream("h264", 1920, 1080)]));
    assert_eq!(c.verdict, Reject);
    assert_eq!(c.reason, "Ролик длиннее минуты — заставка должна быть короткой. Обрежьте его или выберите другой.");
    let c = check(webm, Some("4"), json!([vstream("vp9", 450, 450)]));
    assert_eq!(c.verdict, Reject);
    assert_eq!(
        c.reason,
        "Картинка слишком маленькая (450×450) — на экране она будет мутной. Выберите ролик не меньше 640×360."
    );
    assert_eq!(check(webm, Some("4"), json!([vstream("vp9", 640, 359)])).verdict, Reject);

    // Boundaries: exactly 1 s, exactly 60 s and 640×360 are accepted.
    assert_eq!(check(webm, Some("1"), json!([vstream("vp9", 640, 360)])).verdict, AsIs);
    let c = check(webm, Some("60"), json!([vstream("vp9", 640, 360)]));
    assert_eq!(c.verdict, AsIs);
    // Longer than 30 s — a warning, but not a refusal.
    assert_eq!(
        c.warnings,
        vec!["Ролик длиннее 30 секунд — это заставка, а не фильм: ждать её придётся при каждом входе.".to_string()]
    );
    assert!(check(webm, Some("30"), json!([vstream("vp9", 640, 360)])).warnings.is_empty());
    let c = check(mp4, Some("45"), json!([vstream("h264", 1920, 1080)]));
    assert_eq!((c.verdict, c.warnings.len()), (Transcode, 1));
    assert_eq!(
        ss::msg_movie_will_convert(),
        "Ролик будет перекодирован в WebM, чтобы его мог показать Steam. Исходный файл останется как есть."
    );
}

/// The video is checked by the same ffprobe as the duration for the showcase.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_probe_movie_runs_ffprobe() {
    let _lang = i18n_pin::ru();
    let out = probe_json("matroska,webm", Some("4.0"), json!([vstream("vp9", 1280, 720)]));
    let mut io = FakeIo::new()
        .with_bin("ffprobe")
        .with_run("-show_streams", 0, &out, "");
    let c = Splash::new(&mut io, HOME).probe_movie("/v.webm");
    assert_eq!(c.verdict, ss::MovieVerdict::AsIs);
    assert!(io.has("run /usr/bin/ffprobe -v error -show_streams -show_format -of json /v.webm"));
    // No ffprobe or it crashed — a refusal, not a silent acceptance.
    let mut io = FakeIo::new();
    assert_eq!(Splash::new(&mut io, HOME).probe_movie("/v.webm").verdict, ss::MovieVerdict::Reject);
    let mut io = FakeIo::new().with_bin("ffprobe").with_run("-show_streams", 1, "", "Invalid data");
    assert_eq!(Splash::new(&mut io, HOME).probe_movie("/v.webm").verdict, ss::MovieVerdict::Reject);
}

/// Real small videos made by ffmpeg in a temporary directory — parsing
/// live ffprobe output, not only samples.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: ffmpeg и ffprobe")]
fn movie_probe_on_real_clips() {
    use std::process::Command;
    let have = |b: &str| Command::new(b).arg("-version").output().map(|o| o.status.success()).unwrap_or(false);
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("ffmpeg/ffprobe нет — проверка пропущена");
        return;
    }
    let dir = std::env::temp_dir().join(format!("rexon-movie-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let make = |name: &str, size: &str, secs: &str, codec: &[&str]| {
        let path = dir.join(name);
        let mut args: Vec<String> = ["-v", "error", "-y", "-f", "lavfi", "-i"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.push(format!("testsrc=size={size}:rate=10:duration={secs}"));
        args.extend(["-f", "lavfi", "-i"].iter().map(|s| s.to_string()));
        args.push(format!("sine=duration={secs}"));
        args.extend(codec.iter().map(|s| s.to_string()));
        args.push("-shortest".into());
        args.push(path.to_string_lossy().into_owned());
        let ok = Command::new("ffmpeg").args(&args).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok, "ffmpeg не сделал {name}");
        let argv = ss::ffprobe_streams_argv("ffprobe", &path.to_string_lossy());
        let out = Command::new(&argv[0]).args(&argv[1..]).output().unwrap();
        ss::check_movie_probe(&String::from_utf8_lossy(&out.stdout))
    };
    let vp9 = ["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libopus"];
    let c = make("ok.webm", "640x360", "2", &vp9);
    assert_eq!(c.verdict, ss::MovieVerdict::AsIs, "{c:?}");
    assert!((c.duration - 2.0).abs() < 0.2, "{c:?}");
    let c = make("small.webm", "320x240", "2", &vp9);
    assert_eq!(c.verdict, ss::MovieVerdict::Reject, "{c:?}");
    let c = make("other.mkv", "640x360", "2", &["-c:v", "mpeg4", "-c:a", "mp2"]);
    assert_eq!(c.verdict, ss::MovieVerdict::Transcode, "{c:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// New captions in English — without Russian.
#[test]
fn custom_movie_texts_in_english() {
    let _lang = i18n_pin::en();
    assert_eq!(ss::custom_movie_label(), "Your own movie");
    for text in [
        ss::msg_movie_unreadable(),
        ss::msg_movie_no_picture(),
        ss::msg_movie_too_short(),
        ss::msg_movie_too_long(),
        ss::msg_movie_too_small(320, 240),
        ss::msg_movie_long_warning(),
        ss::msg_movie_will_convert(),
    ] {
        assert!(!text.is_empty() && !has_cyrillic(&text), "{text}");
    }
    assert!(ss::msg_movie_too_small(320, 240).contains("(320×240)"));
}
