//! Golden-сверка `session_splash.rs` (заставка входа).
//!
//! `golden/session_splash_cases.json` — эталон. Когда-то он был снят
//! read-only с живого `rexon.session_splash` одноразовым генератором
//! (HOME/XDG во временном каталоге, subprocess/socket/X11-оверлей замоканы,
//! mpv/ffmpeg/systemctl/kwriteconfig6/qdbus и живой KWin не запускались, пути
//! нормализованы в `/HOME` и `/usr/local/lib/rexon`). С 02.10.2026 Python в
//! Rexon — только архив: снимок заморожен и служит СОБСТВЕННЫМ эталоном
//! Rust. Покрыто: константы/пути, скан заставок Steam (встроенные +
//! инвентарь, имена из localconfig.vdf), конфиг (чтение/слияние/
//! сериализация), гибрид (тема LnF, ksplashrc, argv kwriteconfig6/systemctl/
//! ffmpeg, systemd-юнит, решение hybrid_supported), правило KWin
//! (создание/миграция/идемпотентность + багфикс регистрации),
//! reconfigure-кандидаты, разбор xrandr, mpv-IPC (payload'ы и разбор
//! ответа), sd_notify, кандидаты/argv rexon-splash-overlay, argv mpv всех
//! веток `play`, маршруты install/uninstall autostart, выбор постера,
//! тексты сообщений.
//!
//! Места, где Rust сознательно ушёл от снятого поведения, — ТОЛЬКО
//! осознанные починки (см. шапку модуля), каждая закреплена здесь отдельным
//! тестом.

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
// Константы и пути
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
        // Имя — msgid; эталон Python — русский, сверяем перевод из ru.po.
        assert_eq!(g["builtin_names"][*key], Value::from(rexon_core::i18n::tr(name)));
    }
    assert_eq!(
        ss::BUILTIN_NAMES.len(),
        g["builtin_names"].as_object().unwrap().len()
    );
    // производные пути runtime
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

/// Починка №5: корень пользовательских конфигов уважает `$XDG_CONFIG_HOME`.
/// В оригинале переменную знал только `_ksplashrc_get`, остальные пути
/// вшивали `~/.config` — при нестандартном значении systemd-юнит и
/// `.desktop` уезжали мимо тех каталогов, где их ищут (автозапуск заставки
/// молча не включался), а правило KWin писалось мимо живого `kwinrulesrc`.
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
    // Пустая переменная = не задана (Python-истинность), хвостовой слэш
    // не даёт двойного разделителя — корень общий с we_client::config_home.
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
    // ⛔РАСХОЖДЕНИЕ С ЭТАЛОНОМ, НАЗВАННОЕ ВСЛУХ (16.09.2026, блок H).
    // Оригинал без лаунчера в `PATH` выдавал `launcher_exec_fallback` —
    // `<python> -m rexon.app --session-splash`. Эта строка уезжала в
    // `Exec=` автозапуска и в `ExecStart=` юнита, где выполняется без
    // рабочего каталога, а пакета `rexon` в `sys.path` системного
    // интерпретатора нет: заставка при входе молча не показывалась.
    // Теперь заводить нечем — значит пусто, и вызывающий отказывает вслух
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
    // Переснято 30.09.2026: `Comment` — английский + `Comment[<код>]` из
    // каталога (прежде был только русский, без ключа языка).
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
// Steam: имена из VDF + скан заставок
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
    // листинг НЕсортированный — сортировка внутри, как sorted() в Python
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
    ));
    // Починка №3: у файла без «_» id — из стема, а не из полного имени
    // с расширением (Python давал id/имя «weird.webm»).
    let mut expected = g["scan_movies_python"].clone();
    {
        let arr = expected.as_array_mut().unwrap();
        let weird = arr.last_mut().unwrap();
        assert_eq!(weird["id"], "weird.webm"); // квирк Python в эталоне
        weird["id"] = "weird".into();
        weird["name"] = "Инвентарь · weird".into();
    }
    assert_eq!(out, expected);
}

// ============================================================================
// Конфиг
// ============================================================================

#[test]
fn config_roundtrip() {
    let g = fx();
    let initial = r#"{"path": "/old.webm", "sound": false, "prev_ksplash_theme": "Breeze"}"#;
    assert_eq!(ss::parse_config(Some(initial)), g["load_config_ok"]);
    assert_eq!(ss::parse_config(Some("{broken")), g["load_config_broken"]);
    assert_eq!(ss::parse_config(Some("[1, 2]")), g["load_config_nondict"]);
    assert_eq!(ss::parse_config(None), g["load_config_missing"]);

    // save_config: обновление path/sound без потери служебных ключей
    let cfg = ss::parse_config(Some(initial));
    let saved = ss::config_with_selection(&cfg, "/usr/local/share/movie.webm", false);
    assert_eq!(ss::config_json(&saved), s(&g["save_config_text"]));
    // save_hybrid_pref поверх сохранённого
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
// Гибрид: тема LnF, ksplashrc, argv, решение поддержки
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn lnf_theme_texts() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ss::SPLASH_QML, s(&g["splash_qml"]));
    assert_eq!(ss::SPLASH_QML, s(&g["lnf_splash_qml"]));
    // Переснято 30.09.2026: описание темы — английское + `Description[<код>]`
    // из каталога (прежде было только русское, без ключа языка).
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
    assert!(ss::should_backup_prev_theme("")); // ключа не было — тоже бэкап
    assert!(!ss::should_backup_prev_theme(ss::KSPLASH_THEME_ID));
    assert!(ss::should_rollback_ksplashrc(ss::KSPLASH_THEME_ID));
    assert!(!ss::should_rollback_ksplashrc("org.kde.breeze.desktop"));
}

// ============================================================================
// Правило KWin
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_texts() {
    // Эталон снят с русским названием правила.
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(ss::KWIN_RULE_DESC, s(&g["kwin_rule_desc"]));
    assert_eq!(ss::KWIN_RULE_FSP, s(&g["kwin_rule_fsp"]));
    assert_eq!(ss::kwin_rule_body(), s(&g["kwin_rule_body"]));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_create_empty() {
    // Эталон снят с русским названием правила.
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
    // Эталон снят с русским названием правила.
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
    // Эталон снят с русским названием правила.
    let _lang = i18n_pin::ru();
    let g = fx();
    // актуальное правило уже есть → файл не перезаписывается
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(s(&g["kwin_uptodate"]["initial"]))),
        None
    );
    assert!(!g["kwin_uptodate"]["changed"].as_bool().unwrap());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn kwin_rule_migrate_old() {
    // Эталон снят с русским названием правила.
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
    // Эталон снят с русским названием правила.
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
    // Эталон снят с русским названием правила.
    let _lang = i18n_pin::ru();
    let g = fx();
    // Починка №1: [General] без rules=/count= — Python дописывал блок, но НЕ
    // регистрировал его (re.sub вхолостую → KWin правило игнорирует); Rust
    // вписывает недостающие ключи сразу после заголовка [General].
    let input = s(&g["kwin_broken_general_input"]);
    let fixed = format!(
        "[1]\nwmclass=a\n\n[General]\ncount=1\nrules=2\nsomething=1\n\n[2]\n{}",
        ss::kwin_rule_body()
    );
    let out = ss::ensure_kwin_rule_text(Some(input)).unwrap();
    assert_eq!(out, fixed);
    // ...и это отличается от эталонного (багованного) вывода Python.
    assert_ne!(out, s(&g["kwin_broken_general_python"]));
    assert!(!s(&g["kwin_broken_general_python"]).contains("rules="));
}

// ----------------------------------------------------------------------------
// Название правила переводится, а уже стоящее правило находится и при смене
// языка (01.10.2026). Название видно человеку в «Правилах окон» KDE; до
// перевода оно было русским для всех.
// ----------------------------------------------------------------------------

/// Правило в том виде, как его пишет Rexon, с заданным названием.
fn our_rule(n: &str, title: &str, with_fsp: bool) -> String {
    let body = ss::kwin_rule_body();
    let tail = body.split_once('\n').unwrap().1; // всё после строки Description=
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

/// Старое русское правило у человека, который перешёл на английский:
/// находится (второго правила не появляется) и переименовывается.
#[test]
fn kwin_rule_old_russian_found_and_renamed() {
    let _lang = i18n_pin::en();
    let en = "Rexon: startup movie on top of everything";
    let ru = "Rexon: заставка входа поверх всего";
    let before = format!("{FOREIGN}{}{}", our_rule("2", ru, true), general("1,2", 2));
    let after = ss::ensure_kwin_rule_text(Some(&before)).expect("название переписывается");
    assert_eq!(after, format!("{FOREIGN}{}{}", our_rule("2", en, true), general("1,2", 2)));
    // Повторный запуск ничего не меняет — файл не переписывается каждый раз.
    assert_eq!(ss::ensure_kwin_rule_text(Some(&after)), None);

    // Совсем старое русское правило без снятия FSP: обе правки сразу.
    let old = format!("{}{}", our_rule("1", ru, false), general("1", 1));
    assert_eq!(
        ss::ensure_kwin_rule_text(Some(&old)).unwrap(),
        format!("{}{}", our_rule("1", en, true), general("1", 1))
    );
}

/// Правило с английским названием у человека с русским языком.
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

/// Немецкое название — из НАСТОЯЩЕГО `po/de.po` (собран msgfmt): правило
/// находится по переводу и без `wmclass` (поправлено руками), второго
/// блока не появляется.
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
    // wmclass снят руками — узнаём по названию.
    let rule = our_rule("3", &de, true).replace("wmclass=rexon-session-splash\nwmclassmatch=1\n", "");
    let before = format!("{FOREIGN}{rule}{}", general("1,3", 2));
    let after = ss::ensure_kwin_rule_text_with(Some(&before), en, &known).unwrap();
    assert_eq!(after, before.replace(&format!("Description={de}\n"), &format!("Description={en}\n")));
    assert_eq!(after.matches("[General]").count(), 1);
    assert_eq!(after.matches("\n[").count(), before.matches("\n[").count(), "второго правила нет");
}

/// Чужие правила не трогаются: ни похожий `wmclass`, ни своё название,
/// данное человеком нашему правилу.
#[test]
fn kwin_rule_foreign_untouched() {
    let _lang = i18n_pin::en();
    let en = "Rexon: startup movie on top of everything";
    // `wmclass=rexon-session-splash-x` — не наше: раньше сошло бы подстрокой.
    let lookalike = "[2]\nDescription=Чужое\nwmclass=rexon-session-splash-x\nwmclassmatch=1\n";
    let before = format!("{FOREIGN}{lookalike}{}", general("1,2", 2));
    let after = ss::ensure_kwin_rule_text(Some(&before)).expect("наше правило добавляется");
    assert!(after.starts_with(&format!("{FOREIGN}{lookalike}")), "чужие секции изменены:\n{after}");
    assert!(after.contains(&format!("Description={en}\n")));

    // Наше правило, переименованное человеком: название — его, не трогаем.
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
    // Починка №2: валидный JSON-не-объект в потоке ронял Python
    // (AttributeError мимо `except OSError`); здесь строка пропускается.
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
// X11: xrandr, тип сессии
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
// Прогон: хелпер, argv mpv, сообщения
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
    // хелпер запускается без --mute/--paused (звук вкл, гибрид авто-выкл)
    assert_eq!(
        ss::splash_overlay_argv("/usr/local/bin/rexon-splash-overlay", VIDEO, SOCK, true, false),
        str_vec(&case["helper_argv"])
    );
    // умер до IPC-сокета → фолбэк на оконный mpv (Wayland-окно, без гибрида)
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
    // effective_hybrid: None → авто (гибрид установлен И blocking)
    assert!(ss::effective_hybrid(None, true, true));
    assert!(!ss::effective_hybrid(None, true, false));
    assert!(!ss::effective_hybrid(None, false, true));
    assert!(!ss::effective_hybrid(Some(false), true, true));
    assert!(ss::effective_hybrid(Some(true), false, false));
    // хелпер — только реальный прогон на Wayland
    assert!(ss::wants_wayland_helper(true, Some(":0"), Some("wayland-0")));
    assert!(ss::wants_wayland_helper(true, None, Some("wayland-0")));
    assert!(!ss::wants_wayland_helper(false, None, Some("wayland-0")));
    assert!(!ss::wants_wayland_helper(true, Some(":0"), None));
    assert!(!ss::wants_wayland_helper(true, Some(":0"), Some("")));
    // is_enabled: любой из двух механизмов
    assert!(ss::is_enabled(true, false));
    assert!(ss::is_enabled(false, true));
    assert!(!ss::is_enabled(false, false));
}

// ============================================================================
// Autostart: маршруты и сообщения
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn autostart_routes() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let r = &g["autostart_routes"];
    // нет пути → отказ
    assert_eq!(ss::autostart_route(&json!({}), true), ss::AutostartRoute::NoPath);
    assert_eq!(r["no_path"]["result"], json!([false, ss::msg_choose_first()]));
    // путь есть, галка по умолчанию, гибрид поддержан → гибрид
    let cfg = json!({"path": VIDEO, "sound": true});
    assert_eq!(ss::autostart_route(&cfg, true), ss::AutostartRoute::Hybrid);
    assert_eq!(r["hybrid_ok"]["result"], json!([true, ss::msg_hybrid_route_ok()]));
    assert_eq!(
        r["hybrid_ok"]["calls"],
        json!(["supported", "install_hybrid", "rm_desktop"])
    );
    // гибрид не удался → откат + фаза 1 с составным сообщением
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
    // галка выключена + остатки гибрида → чистая фаза 1 с откатом
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
    // гибрид не поддержан, остатков нет → просто фаза 1
    assert_eq!(
        ss::autostart_route(&cfg, false),
        ss::AutostartRoute::Plain { rollback_hybrid: false }
    );
    // uninstall: сообщение одно на оба пути
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
// Выбор постера
// ============================================================================

#[test]
fn poster_selection() {
    let g = fx();
    let cfg = json!({"path": VIDEO, "sound": true});
    // ролик из конфига + постер темы на месте → готовый постер темы
    assert_eq!(
        ss::poster_source(&cfg, VIDEO, true, HOME, RUN),
        ss::PosterSource::Theme(s(&g["poster_theme_hit"]).to_string())
    );
    // темы нет → свежий кадр ffmpeg-ом в runtime
    assert_eq!(
        ss::poster_source(&cfg, VIDEO, false, HOME, RUN),
        ss::PosterSource::Extract(s(&g["poster_runtime"]).to_string())
    );
    // другой ролик → тоже извлечение, даже при живом постере темы
    assert_eq!(
        ss::poster_source(&cfg, "/HOME/other.webm", true, HOME, RUN),
        ss::PosterSource::Extract(s(&g["poster_runtime"]).to_string())
    );
    assert!(g["poster_fail"].is_null()); // неудача ffmpeg → None (исполнитель)
}

// ============================================================================
// Витрина заставок: метаданные предмета, кадр, избранное, ротация
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
    // Запись отдаётся ЦЕЛИКОМ (обложка, item_class, локализованный
    // заголовок) — витрине нужны все поля, в отличие от маршрута скана.
    assert_eq!(
        ss::parse_startup_movie_items(s(&g["vdf_text"])),
        g["items"]
    );
    // Числовой communityitemid становится строковым ключом, как str() в Python.
    let items = ss::parse_startup_movie_items(s(&g["vdf_text"]));
    assert!(items.get("991").is_some());
    // Отброшены: null, «none» и элемент-не-словарь.
    assert_eq!(items.as_object().unwrap().len(), 3);
    assert!(items.get("None").is_none());
    assert!(items.get("none").is_none());
    // Нет ключа GetStartupMovies / битый JSON → пусто.
    assert_eq!(ss::parse_startup_movie_items("\"Other\"\t\"1\"\n"), json!({}));
    assert_eq!(
        ss::parse_startup_movie_items("\"GetStartupMovies\"\t\"[не json\"\n"),
        json!({})
    );
}

#[test]
fn appinfo_names_binary_vdf() {
    let g = sc();
    // Версия со строковой таблицей (0x07564429) — та, что реально лежит у
    // современного клиента Steam.
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
    // Незнакомая версия → пусто (в Python — тоже, с записью в журнал).
    let bad = hex_bytes(&g["appinfo_bad_bytes"]);
    assert!(ss::parse_appinfo_names(&bad, &want).is_empty());
    // Пустой запрос не читает файл вообще.
    let data = hex_bytes(&g["appinfo_v29_bytes"]);
    assert!(ss::parse_appinfo_names(&data, &[]).is_empty());
    assert!(ss::parse_appinfo_names(&data, &[0]).is_empty());
    // Обрезанный файл не паникует (Python ловил struct.error).
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
    // Ключевые правила витрины закреплены отдельно от общего сравнения:
    // локализованный item_title важнее английского name (правило #13),
    // ведущий слэш обложки снимается, appid приходит и строкой, и числом.
    assert_eq!(got[1]["title"], json!("Специальная доставка"));
    assert_eq!(got[1]["image"], json!("items/1259420/big.png"));
    assert_eq!(got[1]["game"], json!("Days Gone"));
    assert_eq!(got[2]["appid"], json!(570)); // в VDF было "570" строкой
    assert_eq!(got[2]["title"], json!("Only Name")); // item_title нет → name
    assert_eq!(got[0]["origin"], json!(ss::ORIGIN_BUILTIN));
    assert_eq!(got[4]["appid"], json!(0)); // метаданных нет вовсе
    assert_eq!(got[4]["title"], json!("Без метаданных")); // остаётся имя скана
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
    // Подписи происхождения — те же строки, что в Python.
    let labels = &g["origin_labels"];
    for id in [ss::ORIGIN_BUILTIN, ss::ORIGIN_SHOP, ss::ORIGIN_CUSTOM] {
        assert_eq!(ss::origin_label(id), s(&labels[id]));
    }
    assert_eq!(
        str_vec(&g["origins"]),
        vec![ss::ORIGIN_BUILTIN, ss::ORIGIN_SHOP, ss::ORIGIN_CUSTOM]
    );
    // Неизвестное происхождение → пустая подпись (пустую метку не рисуем).
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
    // Не-строки в favorites отбрасываются, как isinstance-фильтр Python.
    assert_eq!(
        ss::favorites_from_config(&json!({"favorites": ["/a", 5, null, "/b"]})),
        vec!["/a".to_string(), "/b".to_string()]
    );
    // Не-список и отсутствие ключа → пусто.
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

    // Пул: избранное от двух штук, иначе всё найденное (правило #13 —
    // одна звёздочка не должна давать «один и тот же ролик каждый вход»).
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

    // Кандидаты жребия: без текущего, а если после фильтра пусто — весь пул.
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
    // Пул меньше двух → крутить нечего (в Python это ранний return None).
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
    // seek ≤ 0.05 → без -ss (тот же argv, что у повторной попытки).
    assert_eq!(
        ss::preview_ffmpeg_argv(exe, VIDEO, tmp, 0.0),
        str_vec(&a["no_seek"])
    );
    assert_eq!(
        ss::preview_ffmpeg_argv(exe, VIDEO, tmp, 0.04),
        str_vec(&a["tiny_seek"])
    );

    // Каталог кэша уважает $XDG_CACHE_HOME.
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
    // ⚠️ Починка №6: отпечаток в имени файла кэша ДЕТЕРМИНИРОВАННЫЙ.
    // Питоновский `hash((abspath, mtime))` солится PYTHONHASHSEED, поэтому
    // байт-паритета с оригиналом тут быть НЕ МОЖЕТ — эталон нестабилен
    // between запусками. Сверяем то, что от оригинала осталось (санитайзер
    // имени) и главное свойство починки: одинаковый вход → одинаковое имя,
    // разный mtime/путь → разное.
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
// Потоки заставки: реплей журнала исполнителя (шов `SplashIo`)
// ============================================================================
//
// Второй слой порта: ядро ведёт САМИ ПОТОКИ (скан, конфиг, гибрид, автозапуск
// и все три пути прогона), а побочные эффекты исполняет трейт. Здесь трейт
// подменён журналирующей заглушкой: проверяется ПОРЯДОК шагов и решения на
// развилках — то, что раньше жило в Python-`play`/`_install_hybrid` и никакой
// сверкой не покрывалось (записанный эталон Python-пути их не трогал: живой
// mpv/ffmpeg/systemctl в тесте не запустишь).
//
// `sleep` двигает фальшивые часы — иначе ожидания с дедлайнами не кончались бы.

use rexon_core::session_splash::{RunOut, Splash, SplashIo};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct FakeIo {
    env: HashMap<String, String>,
    bins: HashMap<String, String>,
    files: HashSet<String>,
    dirs: HashSet<String>,
    texts: HashMap<String, String>,
    /// подстрока argv → (код возврата, stdout, stderr)
    runs: Vec<(String, i64, String, String)>,
    written: HashMap<String, String>,
    journal: Vec<String>,
    now: f64,
    /// на каком по счёту poll процесс считается завершённым
    exit_after_polls: Option<usize>,
    exit_code: i64,
    polls: usize,
    overlay: bool,
    compositor: bool,
    spawn_err: Option<String>,
    ipc_reply: Option<String>,
    lock_free: bool,
    steps: HashMap<String, (bool, String)>,
    /// Выбранный вход продукта; `None` — Python-вход (прежние сверки).
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
        // Успешный ffmpeg оставляет файл последним аргументом argv —
        // дальше поток проверяет его существование.
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

/// Ролик из фальшивого дерева Steam (общие HOME/VIDEO — выше).
const FLOW_VIDEO: &str = "/HOME/.local/share/Steam/steamui/movies/deck_startup.webm";

/// Конфиг «ролик выбран» уже на месте.
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
    // ни замка, ни процессов: отказ ДО первого побочного действия
    assert!(io.journal.iter().all(|l| !l.starts_with("spawn")), "{:?}", io.journal);
    assert!(!io.has("lock "));
}

#[test]
fn flow_play_second_run_skipped_by_lock() {
    let _lang = i18n_pin::en();
    let mut io = with_config(FakeIo::new().with_file(FLOW_VIDEO).with_bin("mpv"), "");
    io.lock_free = false;
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    // «уже проигрывается» — успех: второй экземпляр молча уходит
    assert!(ok);
    assert_eq!(msg, ss::msg_already_playing());
    assert!(io.journal.iter().all(|l| !l.starts_with("spawn")));
}

#[test]
fn flow_play_wayland_helper_relay_order() {
    let _lang = i18n_pin::en();
    // Гибрид на Wayland: хелпер поднялся, ksplashqml погас, mpv распаузили.
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
            .with_run("pgrep", 1, "", ""), // ksplashqml не бежит
        ", \"hybrid_installed\": true",
    );
    io.ipc_reply = Some("{\"request_id\":700,\"error\":\"success\",\"data\":true}\n".into());
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    assert_eq!(msg, ss::msg_played());
    // Порядок: замок → запуск хелпера → READY=1 → снятие паузы → quit.
    let i_lock = io.index_of("lock ");
    let i_spawn = io.index_of("spawn[false] /repo/rust/target/release/rexon-splash-overlay");
    let i_ready = io.index_of("notify /run/systemd/notify READY=1");
    let i_unpause = io.index_of("ipc {\"command\": [\"set_property\", \"pause\", false]}");
    let i_quit = io.index_of("ipc {\"command\": [\"quit\"]}");
    assert!(i_lock < i_spawn && i_spawn < i_ready && i_ready < i_unpause && i_unpause < i_quit,
            "{:#?}", io.journal);
    // Оконный mpv не запускался — хелпер справился сам.
    assert!(!io.has("spawn[false] mpv"), "{:#?}", io.journal);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_helper_dead_falls_back_to_window_mpv() {
    let _lang = i18n_pin::ru();
    // Хелпер умер до IPC-сокета → прозрачный откат на оконный mpv + правило KWin.
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
    io.exit_after_polls = Some(0); // хелпер мёртв на первой же пробе
    let (ok, _msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok);
    assert!(io.has(ss::MSG_HELPER_NO_LIFTOFF), "{:#?}", io.journal);
    let cmd = io
        .journal
        .iter()
        .find(|l| l.starts_with("spawn[false] mpv"))
        .expect("оконный mpv не запущен");
    assert!(cmd.contains("--wayland-app-id=rexon-session-splash"), "{cmd}");
    // правило KWin ставится ДО запуска окна
    assert!(io.index_of("write /HOME/.config/kwinrulesrc") < io.index_of("spawn[false] mpv"));
}

#[test]
fn flow_play_x11_overlay_poster_before_map() {
    let _lang = i18n_pin::en();
    // X11: постер кладётся в слой ДО маппинга, эстафета гибрида не нужна.
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
    // окно mpv рисует внутрь слоя
    assert!(io.has("--wid=88080391"), "{:#?}", io.journal);
    // на X11 гибрид принудительно снят: паузы (и распаузивания) нет
    assert!(!io.has("--pause"), "{:#?}", io.journal);
    assert!(!io.has("set_property"), "{:#?}", io.journal);
    // финал: сначала гаснет слой, потом mpv
    assert!(io.index_of("x11_close") < io.index_of("ipc {\"command\": [\"quit\"]}"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_x11_cold_start_waits_for_compositor() {
    let _lang = i18n_pin::ru();
    // Холодный вход: композитора ещё нет — ждём его под чёрным слоем.
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

/// Путь пользовательского ksplashrc в фальшивом HOME.
const KSPLASHRC: &str = "/HOME/.config/ksplashrc";
/// Каталог LnF-темы гибрида в фальшивом HOME.
const THEME_DIR: &str = "/HOME/.local/share/plasma/look-and-feel/org.rexon.sessionsplash";

/// Фикстура «гибрид стоит, вход идёт по-настоящему» (Wayland + хелпер).
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

/// Решение самолечения: лечим только когда гибрид числится установленным и
/// в ksplashrc стоит НЕ наша тема (пусто = ключ потеряли).
#[test]
fn heal_ksplashrc_decision() {
    let on = json!({"hybrid_installed": true});
    let off = json!({"hybrid_installed": false});
    let empty = json!({});
    // гибрид стоит, тема чужая либо ключ потерян → чиним
    assert!(ss::should_heal_ksplashrc(&on, "org.kde.breezedark.desktop"));
    assert!(ss::should_heal_ksplashrc(&on, ""));
    // гибрид стоит, тема уже наша → не трогаем
    assert!(!ss::should_heal_ksplashrc(&on, ss::KSPLASH_THEME_ID));
    // гибрида нет → чужой ksplashrc не наше дело ни при каком значении
    assert!(!ss::should_heal_ksplashrc(&off, "org.kde.breezedark.desktop"));
    assert!(!ss::should_heal_ksplashrc(&off, ""));
    assert!(!ss::should_heal_ksplashrc(&empty, "org.kde.breezedark.desktop"));
    assert!(!ss::should_heal_ksplashrc(&empty, ss::KSPLASH_THEME_ID));
}

/// Спор за слот KSplash: видеозаставка входа и заставка KDE — одно место, и
/// выбирать обязан покупатель. Проверяем ВСЕ ветки решения: молчание в обе
/// стороны здесь и есть дефект.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn splash_slot_conflict_branches() {
    let _lang = i18n_pin::ru();
    // Гибрида нет — слот свободен, спрашивать не о чем ни при каком виде.
    assert!(ss::splash_slot_conflict("splash", "Breeze", false).is_none());
    assert!(ss::splash_slot_conflict("global", "Breeze", false).is_none());
    // Вид оформления слота не касается — тоже молча применяем.
    for kind in ["colors", "cursor", "icons", "gtk", "lockscreen", "wallpapers", "none"] {
        assert!(!ss::apply_kind_takes_ksplash_slot(kind), "{kind}");
        assert!(ss::splash_slot_conflict(kind, "Breeze", true).is_none(), "{kind}");
    }
    // Наша же тема-постер: отбирать место не у кого.
    assert!(ss::splash_slot_conflict("splash", ss::KSPLASH_THEME_ID, true).is_none());
    assert!(ss::splash_slot_conflict("global", ss::KSPLASH_THEME_ID, true).is_none());

    // Чужая заставка KDE поверх гибрида — вопрос, и «Нет» означает «не
    // применять»: у вида `splash` всё действие и есть заставка.
    assert!(ss::apply_kind_takes_ksplash_slot("splash"));
    let c = ss::splash_slot_conflict("splash", "Breeze", true).expect("вопрос обязан быть");
    assert_eq!(c.title, ss::slot_question_title());
    assert!(!c.keep_applies_rest);
    assert!(c.question.contains("Breeze"), "{}", c.question);
    // Оба исхода названы в тексте прямым словом, иначе «Да»/«Нет» — лотерея.
    assert!(c.question.contains("«Да»") && c.question.contains("«Нет»"), "{}", c.question);
    assert_eq!(c.kept, ss::msg_slot_kept_instead_of("Breeze"));
    assert!(c.kept.contains("не применена"), "{}", c.kept);

    // Целое оформление: заставка — лишь одна его часть, поэтому «Нет»
    // применяет ОСТАЛЬНОЕ, а слот остаётся за роликом.
    assert!(ss::apply_kind_takes_ksplash_slot("global"));
    let g = ss::splash_slot_conflict("global", "Nordic", true).expect("вопрос обязан быть");
    assert_eq!(g.title, ss::slot_question_title());
    assert!(g.keep_applies_rest);
    assert!(g.question.contains("Nordic"), "{}", g.question);
    assert!(g.question.contains("«Да»") && g.question.contains("«Нет»"), "{}", g.question);
    assert_eq!(g.kept, ss::msg_slot_kept_video());
}

/// Тексты обоих исходов — по-человечески и без жаргона: имена технологий
/// латиницей, «гибрид KSplash» покупателю не показываем.
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
    // Отказ обязан объяснить, ПОЧЕМУ заставку не применили, а не просто
    // сказать «не вышло».
    assert!(failed.contains("не применена"), "{failed}");
    assert!(released.contains("выключена"), "{released}");
}

/// ⚠️ 09.09.2026: применение глобальной темы Plasma перезаписало
/// `[KSplash] Theme` — при входе секунду висела ШТАТНАЯ заставка Plasma, и
/// только потом выпрыгивал ролик. Реальный прогон обязан вернуть связку
/// САМ, до показа, а чужую тему увести в бэкап для честного отката.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_play_heals_foreign_ksplashrc_theme() {
    let _lang = i18n_pin::ru();
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\nEngine=KSplashQML\n"));
    let (ok, msg) = Splash::new(&mut io, HOME).play(None, None, true, None, "/repo");
    assert!(ok, "{msg}");
    // связка возвращена нашей темой и нашим движком
    let i_theme = io.index_of("--key Theme org.rexon.sessionsplash");
    let i_engine = io.index_of("--key Engine KSplashQML");
    // …и обязательно ДО запуска плеера
    let i_spawn = io.index_of("spawn[false] /repo/rust/target/release/rexon-splash-overlay");
    assert!(i_theme < i_spawn && i_engine < i_spawn, "{:#?}", io.journal);
    // чужая тема ушла в бэкап: откат гибрида вернёт покупателю ЕЁ, не нашу
    let cfg: Value = serde_json::from_str(
        io.written
            .get("/HOME/.config/rexon/session_splash.json")
            .expect("конфиг с бэкапом не записан"),
    )
    .unwrap();
    assert_eq!(cfg["prev_ksplash_theme"], json!("org.kde.breezedark.desktop"));
    assert_eq!(cfg["prev_ksplash_engine"], json!("KSplashQML"));
    // каталог темы был на месте — перегенерации не затевали
    assert!(!io.has("-frames:v"), "{:#?}", io.journal);
}

/// Ключ потеряли целиком (живой случай: ksplashrc оказался ПУСТ) — тогда в
/// бэкап уходит пустая строка, и откат вернёт состояние «ключа не было».
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

/// Связка цела — ksplashrc не трогаем и конфиг не переписываем.
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

/// Каталог темы-постера пропал (снесли `~/.local/share`) — перегенерируем
/// его по ФАКТИЧЕСКОМУ пути ролика, вход не остаётся чёрным.
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

/// Самолечение не роняет показ: kwriteconfig6 отсутствует — жалуемся в
/// журнал, но ролик всё равно играем.
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

/// Тест заставки из UI (non-blocking) — чужой ksplashrc не наше дело:
/// пользователь ничего не входил, связка при входе ни при чём.
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
    // ⚠️ порог, из-за которого была панель поверх заставки на холодном входе
    assert!(text.contains("TimeoutStartSec=15"), "{text}");
    assert!(text.contains("Type=notify") && text.contains("Before=plasma-plasmashell.service"));
    // ksplashrc правится ПОСЛЕ бэкапа прежней темы в конфиг
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
            // Заводить заставку ЕСТЬ чем — проверяем именно отказ ksplashrc,
            // а не отсутствие входа (у него свой сторож ниже).
            .with_bin("rexon")
            .with_run("-frames:v", 0, "", ""),
        "",
    );
    let (ok, msg) = Splash::new(&mut io, HOME).install_hybrid(FLOW_VIDEO, "/usr/bin/python3");
    assert!(!ok);
    assert_eq!(msg, ss::msg_ksplashrc_failed());
    // юнит не пишется, если ksplashrc не поддался — половины гибрида не бывает
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

/// Хуки входа идут по выбору входа продукта: при Rust-входе и юнит гибрида,
/// и autostart-`.desktop` зовут бинарь окна — без интерпретатора, даже когда
/// лаунчер `rexon` стоит в PATH. При Python-входе — прежние строки.
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

/// ⛔Заводить заставку нечем — отказ ВСЛУХ, а не запись в пустоту.
///
/// До 16.09.2026 (блок H) на этой развилке — Python-вход и лаунчера `rexon`
/// в `PATH` нет — обе записи получали `python3 -m rexon.app
/// --session-splash` и рапортовали успех. Ни `.desktop`-автозапуск, ни
/// systemd-юнит не дают рабочего каталога, а пакета `rexon` в `sys.path`
/// системного интерпретатора нет: при входе не показывалось ничего, и
/// узнать об этом было неоткуда.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_login_hooks_refuse_when_nothing_to_launch() {
    let _lang = i18n_pin::ru();
    // Лаунчера `rexon` в PATH НЕТ (`with_bin("rexon")` не вызван).
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
    // Гибрид возможен и встал → .desktop снимается.
    let mut io = with_config(FakeIo::new(), "");
    io.steps.insert("supported".into(), (true, String::new()));
    let (ok, msg) = Splash::new(&mut io, HOME).install_autostart();
    assert!(ok && msg == ss::msg_hybrid_route_ok(), "{msg}");
    assert_eq!(io.steps_of("step "), vec![format!("step hyb {FLOW_VIDEO}"), "step rmdesk".to_string()]);

    // Гибрид упал → полный откат остатков и честная фаза 1.
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

    // Ролик не выбран — ни одного шага.
    let mut io = FakeIo::new();
    let (ok, msg) = Splash::new(&mut io, HOME).install_autostart();
    assert!(!ok && msg == ss::msg_choose_first());
    assert!(io.steps_of("step ").is_empty());
}

#[test]
fn flow_hybrid_supported_short_circuits() {
    let _lang = i18n_pin::en();
    // Не KDE — бинари даже не ищем.
    let mut io = FakeIo::new().with_env("XDG_CURRENT_DESKTOP", "GNOME");
    assert!(!Splash::new(&mut io, HOME).hybrid_supported());
    assert!(io.journal.is_empty(), "{:#?}", io.journal);

    // KDE, но нет ffmpeg — systemctl не дёргаем.
    let mut io = FakeIo::new().with_env("XDG_CURRENT_DESKTOP", "KDE");
    assert!(!Splash::new(&mut io, HOME).hybrid_supported());
    assert!(!io.has("run "), "{:#?}", io.journal);

    // Всё на месте и цель активна.
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
        // первая попытка (со сдвигом) провалилась, вторая — с первого кадра
        .with_run("-ss", 1, "", "битый")
        .with_run("-frames:v", 0, "", "");
    let out = {
        let mut sp = Splash::new(&mut io, HOME);
        sp.preview_frame(video)
    };
    let out = out.expect("кадр не получен");
    assert!(out.starts_with(cache), "{out}");
    let runs = io.steps_of("run ");
    assert_eq!(runs.len(), 3, "{runs:#?}"); // ffprobe + две попытки ffmpeg
    assert!(runs[1].contains("-ss "), "{:?}", runs[1]);
    assert!(!runs[2].contains("-ss "), "{:?}", runs[2]);
    assert!(io.has(&format!("replace {out}.part.png → {out}")));

    // Повторный вызов берёт готовый файл: ffmpeg больше не дёргается.
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
    // текущий ролик из жребия исключён — иначе «случайность» стояла бы на месте
    assert_eq!(nxt.as_deref(), Some(b));
    let cfg = io
        .written
        .get("/HOME/.config/rexon/session_splash.json")
        .expect("конфиг не записан");
    assert!(cfg.contains(b), "{cfg}");
}

#[test]
fn letterbox_rect_matches_mpv_fullscreen() {
    // Широкий ролик в 16:10 экран: поля сверху и снизу.
    assert_eq!(ss::letterbox_rect(1920, 1080, 2560, 1600), (0, 80, 2560, 1440));
    // Вертикальный ролик: поля слева и справа.
    assert_eq!(ss::letterbox_rect(1080, 1920, 2560, 1600), (830, 0, 900, 1600));
    // Точное совпадение пропорций — без полей.
    assert_eq!(ss::letterbox_rect(1280, 800, 2560, 1600), (0, 0, 2560, 1600));
    // Битая картинка (нулевой размер) не роняет слой — заполняем целиком.
    assert_eq!(ss::letterbox_rect(0, 0, 640, 480), (0, 0, 640, 480));
}

/// Гибрид стоит: флаг в конфиге, юнит и тема на месте, откат проходит.
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

/// Поток спора за слот, общий для ОБОИХ хозяев вкладки «Оформление»
/// (Rust-окно и Python-хозяин живого продукта): ветвление «Да/Нет → что
/// делать» живёт в ядре, и здесь проверена каждая ветка.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_resolve_splash_slot_all_answers() {
    let _lang = i18n_pin::ru();
    let unit_rm = "rm /HOME/.config/systemd/user/rexon-session-splash.service";

    // Вид слота не касается — вопроса нет, диск не читается вовсе.
    let mut io = FakeIo::new();
    let plan = Splash::new(&mut io, HOME)
        .resolve_splash_slot("colors", "Breeze", |_| panic!("вопроса быть не должно"));
    assert_eq!(plan, ss::SplashSlotPlan::plain());
    assert!(io.journal.is_empty(), "{:#?}", io.journal);

    // Гибрида нет — спора нет.
    let mut io = FakeIo::new();
    let plan = Splash::new(&mut io, HOME)
        .resolve_splash_slot("splash", "Breeze", |_| panic!("вопроса быть не должно"));
    assert_eq!(plan, ss::SplashSlotPlan::plain());

    // Заставка, «Нет» — не применять, гибрид не трогать.
    let mut io = slot_fixture();
    let mut asked = None;
    let plan = Splash::new(&mut io, HOME).resolve_splash_slot("splash", "Breeze", |c| {
        asked = Some(c.title);
        false
    });
    assert_eq!(asked, Some(ss::slot_question_title()));
    assert_eq!(plan, ss::SplashSlotPlan::Stop(ss::msg_slot_kept_instead_of("Breeze")));
    assert!(!io.has(unit_rm), "{:#?}", io.journal);

    // Целое оформление, «Нет» — применить всё и вернуть слот ролику.
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

    // «Да» — гибрид снят ДО применения, место за выбранной заставкой.
    let mut io = slot_fixture();
    let plan = Splash::new(&mut io, HOME).resolve_splash_slot("splash", "Breeze", |_| true);
    assert_eq!(
        plan,
        ss::SplashSlotPlan::Apply { keep_video: None, note: ss::msg_slot_released() }
    );
    assert!(io.has(unit_rm), "{:#?}", io.journal);
}

/// Доведение плана: слот возвращается ролику, примечание — только к удаче.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn flow_finish_splash_slot_heals_and_notes() {
    let _lang = i18n_pin::ru();
    assert_eq!(ss::msg_with_slot_note("Готово.", "Ролик на месте.", true), "Готово. Ролик на месте.");
    assert_eq!(ss::msg_with_slot_note("", "Ролик на месте.", true), "Ролик на месте.");
    assert_eq!(ss::msg_with_slot_note("Сбой.", "Ролик на месте.", false), "Сбой.");

    // Без ролика — ничего не лечим.
    let mut io = heal_fixture(Some("[KSplash]\nTheme=org.kde.breezedark.desktop\n"));
    let out = Splash::new(&mut io, HOME).finish_splash_slot(None, "", true, "Готово.");
    assert_eq!(out, "Готово.");
    assert!(!io.has("kwriteconfig6"), "{:#?}", io.journal);

    // Ролик оставлен — связка возвращается нашей темой даже при неудаче.
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
// Английский интерфейс
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все сообщения заставки по-английски — без единой русской буквы.
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
        // Самолечение ksplashrc (`msg_heal_*`) пишет только в журнал — его
        // не переводим.
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

/// Английские фразы заставки читаются как фразы, а не калька.
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

/// Те же тексты по-русски — дословно как до перевода на каталог (снимок
/// `ca42e74^`), с «ёлочками».
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

/// Плитка «Нет» в разделе «Заставка» при включённом ролике: вопрос свой —
/// без имени «None», которого человек не видел; без ролика вопроса нет.
#[test]
fn splash_none_tile_asks_in_own_words() {
    assert!(ss::splash_slot_conflict("splash", "None", false).is_none());
    let c = ss::splash_slot_conflict("splash", "None", true).expect("вопрос обязан быть");
    assert!(!c.question.contains("None"), "{}", c.question);
    assert!(!c.kept.contains("None"), "{}", c.kept);
    assert!(!c.keep_applies_rest, "«Нет» — значит не применять");
}
