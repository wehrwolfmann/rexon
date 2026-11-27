//! Check of [`rexon_core::we_window`] — the Wallpaper Engine tab.
//!
//! The expected values are written out VERBATIM (once — from the former
//! `rexon/qt_wallpaper_engine/window.py`, commit `ec8a809`; since 02.10.2026
//! Python in Rexon is only an archive, and these records are Rust's own
//! reference), including the joining of strings broken at column width.
//! Three fixes are checked separately: deleting the temporary file after a
//! failed install, recognising the attempt by the IPC timeout and a single
//! label for the engine version.

use rexon_core::we_window as w;

mod i18n_pin;

// ── Hidden developer mode ───────────────────────────────────────

#[test]
fn dev_env_beats_marker_both_ways() {
    let _lang = i18n_pin::ru();
    // env turns it off even with the marker («as the buyer will see it»)
    for raw in ["0", "false", "no", "NO", " False "] {
        assert!(!w::dev_mode(raw, true), "{raw}");
        assert!(!w::dev_mode(raw, false), "{raw}");
    }
    // env turns it on without the marker
    for raw in ["1", "true", "yes", "YES", " True "] {
        assert!(w::dev_mode(raw, false), "{raw}");
        assert!(w::dev_mode(raw, true), "{raw}");
    }
    // empty/garbage — the marker decides
    for raw in ["", "  ", "maybe", "2"] {
        assert!(w::dev_mode(raw, true), "{raw}");
        assert!(!w::dev_mode(raw, false), "{raw}");
    }
}

#[test]
fn dev_marker_stays_under_config_rexon() {
    assert_eq!(w::DEV_MARKER_PATH, "~/.config/rexon/dev_mode");
    assert_eq!(w::DEV_ENV_VAR, "REXON_DEV");
}

// ── Rail and pages ──────────────────────────────────────────────────

#[test]
fn qml_specs_match_the_vendor_pages() {
    let _lang = i18n_pin::ru();
    let specs = w::qml_page_specs();
    // Fifth in the row is the vendor HEADER flag. It is not here for
    // completeness: a page without a header loses its title and ALL action
    // buttons, while the list is still drawn, and the snapshot looks working.
    // The RULE by which the flag is set is guarded by
    // `we_rail_vendor_parity.rs` — it reads the vendor page itself, not this list.
    let got: Vec<(&str, &str, &str, &str, bool)> = specs
        .iter()
        .map(|i| {
            (
                i.id.as_str(),
                i.icon.as_str(),
                i.label.as_str(),
                i.qml.as_str(),
                i.header,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("wallpapers_qml", "🖼️", "Обои", "WallpaperPage.qml", false),
            ("discover_qml", "🔍", "Обзор", "DiscoverPage.qml", false),
            ("displays_qml", "🖥️", "Мониторы", "DisplaysPage.qml", false),
            ("status_qml", "📈", "Состояние", "StatusPage.qml", false),
            ("plugins_qml", "🧩", "Плагины", "PluginManagePage.qml", true),
            ("settings_qml", "⚙️", "Настройки", "SettingsPage.qml", false),
            ("about_qml", "ℹ️", "О программе", "AboutPage.qml", false),
        ]
    );
}

#[test]
fn rail_index_equals_page_index_in_every_case() {
    let _lang = i18n_pin::ru();
    // QML built, without dev: seven items, seven containers
    let p = w::build_rail(true, false);
    assert_eq!(p.items.len(), 7);
    assert_eq!(p.stub_index, None);
    assert_eq!(p.dev_index, None);
    for i in 0..7 {
        assert!(p.qml_for_index(i).is_some(), "{i}");
        assert!(p.vendor_qml_for_index(i).is_some(), "{i}");
    }

    // QML built, dev: «Dev» as the eighth item, it has no vendor page
    let p = w::build_rail(true, true);
    assert_eq!(p.items.len(), 8);
    assert_eq!(p.dev_index, Some(7));
    assert_eq!(p.items[7].id, "dev");
    // The «Developer» page is OURS (QML module `org.rexon.gui`): the item has a
    // file name, but it has no vendor page and cannot have one.
    assert_eq!(p.qml_for_index(7), Some(w::DEV_PAGE_QML));
    assert_eq!(p.vendor_qml_for_index(7), None);
    assert!(!p.items[7].vendor);

    // QML not built: a stub as page zero
    let p = w::build_rail(false, false);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].id, "no_qml");
    assert_eq!(p.stub_index, Some(0));
    assert_eq!(p.qml_for_index(0), None);
    assert_eq!(p.vendor_qml_for_index(0), None);

    // QML not built + dev: the stub and dev come in a row, the indices do not drift
    let p = w::build_rail(false, true);
    assert_eq!(p.items.len(), 2);
    assert_eq!(p.stub_index, Some(0));
    assert_eq!(p.dev_index, Some(1));
}

#[test]
fn rail_button_label_marks_only_drift() {
    let _lang = i18n_pin::ru();
    assert_eq!(w::rail_button_label("🛠️", "Разраб.", false), "🛠️\nРазраб.");
    assert_eq!(w::rail_button_label("🛠️", "Разраб.", true), "🛠️\nРазраб. ⚠️");
    assert_eq!(w::rail_button_label("🖼️", "Обои", false), "🖼️\nОбои");
    assert_eq!(w::RAIL_WIDTH, 96);
}

// ── Colours and styles ────────────────────────────────────────────────────

#[test]
fn variant_color_matches_the_original_format() {
    assert_eq!(w::variant_color(0, 0, 0), "rgba(0,0,0,0.6)");
    assert_eq!(w::variant_color(255, 255, 255), "rgba(255,255,255,0.6)");
    assert_eq!(w::variant_color(31, 35, 38), "rgba(31,35,38,0.6)");
}

#[test]
fn rail_qss_matches_the_original_concatenation() {
    // ⭐ The rounding here DELIBERATELY diverged from the original's snapshot
    // (`ec8a809` gave 12px): 12 was our invention, while the tab is foreign, and
    // its only yardstick is waywallen. Its rail draws the selection highlight as
    // a «pill», `radius: height / 2` (`RailItem.qml` of the `Qcm.Material`
    // module), so `RAIL_BUTTON_RADIUS` is now computed from the item height:
    // 56 / 2 = 28. The rest of the joining is verbatim as before.
    let dark = w::rail_qss(true, 102, 192, 244);
    assert_eq!(
        dark,
        "QFrame#weRail { background: rgba(255,255,255,0.03); }\
QFrame#weRail QToolButton { border-radius: 28px; font-size: 12px; padding: 4px; }\
QFrame#weRail QToolButton:checked { background: rgba(102,192,244,0.25); }"
    );
    let light = w::rail_qss(false, 37, 99, 235);
    assert!(light.contains("background: rgba(0,0,0,0.02); }"));
    assert!(light.contains("background: rgba(37,99,235,0.25); }"));
}

#[test]
fn rail_background_color_matches_the_qss_alpha() {
    let _lang = i18n_pin::ru();
    // Qt's parser truncates the style sheet fraction: 0.03 → 7/255, 0.02 → 5/255.
    // The QML version takes the same paint as 8-bit, otherwise the rail bar
    // comes out one level lighter than the original (measurement of 04.09.2026).
    assert_eq!(w::rail_background_argb_hex(true), "#07ffffff");
    assert_eq!(w::rail_background_argb_hex(false), "#05000000");
    assert_eq!((w::RAIL_BG_ALPHA_DARK * 255.0) as u8, 7);
    assert_eq!((w::RAIL_BG_ALPHA_LIGHT * 255.0) as u8, 5);
    // And the fractions themselves are those in the original's style sheet.
    assert!(w::rail_qss(true, 1, 2, 3).contains("rgba(255,255,255,0.03)"));
    assert!(w::rail_qss(false, 1, 2, 3).contains("rgba(0,0,0,0.02)"));
}

#[test]
fn rail_selected_color_matches_the_qss_alpha() {
    let _lang = i18n_pin::ru();
    // Qt's parser truncates the same fraction of the selected item highlight:
    // 0.25 → 63/255. The QML version takes the paint as 8-bit, otherwise the
    // highlight comes out one level lighter than the original — (51,82,107)
    // versus (50,82,106) (measurement of 04.09.2026, «Displays» section).
    assert_eq!(w::rail_selected_argb_hex(0x66, 0xc0, 0xf4), "#3f66c0f4");
    assert_eq!(w::rail_selected_argb_hex(37, 99, 235), "#3f2563eb");
    assert_eq!((w::RAIL_SELECTED_ALPHA * 255.0) as u8, 0x3f);
    // And the fraction itself is the one in the original's style sheet.
    assert!(w::rail_qss(true, 0x66, 0xc0, 0xf4)
        .contains("background: rgba(102,192,244,0.25); }"));
}

#[test]
fn logo_and_muted_qss_match() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::logo_qss("#66c0f4"),
        "font-size: 22px; font-weight: 700; color: #66c0f4;"
    );
    // The QML version takes the same numbers as model properties.
    assert_eq!(w::LOGO_FONT_SIZE, 22);
    assert_eq!(w::LOGO_FONT_WEIGHT, 700);
    assert_eq!(
        w::muted_qss("rgba(1,2,3,0.6)", 12),
        "font-size: 12px; color: rgba(1,2,3,0.6);"
    );
    assert_eq!(
        w::muted_qss("rgba(1,2,3,0.6)", 15),
        "font-size: 15px; color: rgba(1,2,3,0.6);"
    );
}

// ── Tab states ────────────────────────────────────────────────

#[test]
fn start_screen_qss_matches_the_original_literals() {
    let _lang = i18n_pin::ru();
    // The original's `ic.setStyleSheet("font-size: 48px;")` and
    // `self._no_daemon_lbl.setStyleSheet("font-size: 15px;")`.
    assert_eq!(w::no_daemon_icon_qss(), "font-size: 48px;");
    assert_eq!(w::no_daemon_text_qss(), "font-size: 15px;");
    assert_eq!(w::STATE_TEXT_FONT_SIZE, 15);
    assert_eq!(w::NO_DAEMON_ICON_FONT_SIZE, 48);
    // The `QVBoxLayout` default under the product style, measurement of 03.09.2026.
    assert_eq!(w::STATE_COLUMN_SPACING, 6);
    assert_eq!(w::STATE_COLUMN_MARGIN, 6);
}

#[test]
fn state_indices_keep_the_original_order() {
    assert_eq!(
        (w::STATE_CONNECTING, w::STATE_NO_DAEMON, w::STATE_PAGES),
        (0, 1, 2)
    );
    assert_eq!(w::HEALTH_TIMEOUT_MS, 5000);
    assert_eq!(w::OWE_IPC_TIMEOUT_MS, 60_000);
    assert_eq!(w::WORKER_WAIT_MS, 60_000);
    assert_eq!(w::STARTUP_DELAY_MS, 100);
}

/// The overall engine wait must be FINITE and LAST.
///
/// Last — because the precise guards fire before it: 12 seconds of bus
/// polling and 5 seconds for the `health` answer. Make it shorter than their
/// sum — and instead of «the engine takes too long to start» the buyer gets
/// generic words, and we lose the diagnosis. The check is here so that this
/// comes out on `cargo test`, not in the live window.
#[test]
fn connect_timeout_outlives_every_inner_guard() {
    let _lang = i18n_pin::ru();
    use rexon_core::we_client as c;
    let polling_ms = i32::try_from(c::POLL_LIMIT * c::POLL_MS as i64).expect("срок поллинга");
    assert_eq!(polling_ms, 12_000);
    assert!(
        w::CONNECT_TIMEOUT_MS > polling_ms + w::HEALTH_TIMEOUT_MS,
        "общий срок ожидания ({} мс) не больше суммы внутренних сторожей \
         ({} + {} мс): он сработает первым и подменит точную причину отказа",
        w::CONNECT_TIMEOUT_MS,
        polling_ms,
        w::HEALTH_TIMEOUT_MS
    );
    // A cold daemon start measured on the owner's machine — 3.2–5.0 s (stand
    // `measure-start.sh`, 18.09.2026). Half a minute of waiting can no longer be
    // explained to the buyer by anything.
    assert!(w::CONNECT_TIMEOUT_MS <= 30_000);
}

#[test]
fn health_verdict_trusts_old_daemons_and_stops_broken_ones() {
    let _lang = i18n_pin::ru();
    assert_eq!(w::health_verdict("healthy"), w::HealthVerdict::Ready);
    assert_eq!(w::health_verdict("HEALTHY"), w::HealthVerdict::Ready);
    // an old daemon without the health field — empty string, we show the pages
    assert_eq!(w::health_verdict(""), w::HealthVerdict::Ready);
    assert_eq!(
        w::health_verdict("degraded"),
        w::HealthVerdict::NotReady(
            "движок ответил, но не готов (состояние: degraded)".to_string()
        )
    );
}

/// Every reason the product can state must be RECOGNISED.
///
/// Otherwise the buyer silently slides to generic words: parsing goes by
/// string, and any rewording of a message in `we_client` breaks it silently —
/// neither the build nor the run will say so. So the messages are taken
/// directly from their authors rather than copied here.
#[test]
fn every_known_reason_is_recognised() {
    let _lang = i18n_pin::ru();
    use rexon_core::we_client as c;
    use w::DaemonFailure as F;
    for (reason, want) in [
        (c::daemon_not_found_message(), F::NotInstalled),
        (
            c::daemon_launch_failed_message("/home/пользователь/waywallen"),
            F::LaunchFailed,
        ),
        (c::daemon_poll_timeout_message(), F::StartTimeout),
        (c::daemon_start_timeout_message(42, true), F::StartTimeout),
        (c::daemon_start_timeout_message(180, false), F::StartTimeout),
        (c::daemon_exited_message(), F::LaunchFailed),
        (w::TEXT_HEALTH_TIMEOUT.to_string(), F::NotResponding),
        (w::REASON_CONNECT_TIMEOUT.to_string(), F::NotResponding),
    ] {
        assert_eq!(w::daemon_failure(&reason), want, "причина «{reason}»");
    }
    let not_ready = match w::health_verdict("degraded") {
        w::HealthVerdict::NotReady(r) => r,
        w::HealthVerdict::Ready => panic!("degraded — не Ready"),
    };
    assert_eq!(w::daemon_failure(&not_ready), F::NotReady);
    assert_eq!(w::daemon_failure("что-то новое"), F::Unknown);
    assert_eq!(w::daemon_failure(""), F::Unknown);
}

/// The refusal screen is for the buyer, and the machine internals are for the log.
///
/// A guard exactly for what the task was started for: paths, `PATH` and the
/// word AppImage filled the whole screen of a person who bought the program
/// for wallpapers.
#[test]
fn failure_screen_says_what_to_do_and_hides_the_machinery() {
    use rexon_core::we_client as c;
    let _lang = i18n_pin::ru();
    let raw = c::daemon_not_found_message();
    for packaged in [false, true] {
        let shown = w::no_daemon_text_in(&raw, packaged);
        for machinery in ["AppImage", "PATH", "/", "~"] {
            assert!(
                !shown.contains(machinery),
                "на экране отказа снова машинная подробность «{machinery}»: {shown}"
            );
        }
    }
    // The same detail must end up in the log — in full and verbatim.
    let line = w::daemon_failure_log_line(&raw).expect("строка журнала");
    assert!(line.contains(&raw), "{line}");
    assert_eq!(w::daemon_failure_log_line("   "), None);

    // Every case says WHAT to do: either with a button or by reinstalling the
    // package — and the package advice sounds ONLY for a package install.
    for reason in [
        c::daemon_launch_failed_message("/opt/waywallen"),
        c::daemon_poll_timeout_message(),
        c::daemon_start_timeout_message(42, true),
        c::daemon_exited_message(),
        w::TEXT_HEALTH_TIMEOUT.to_string(),
        w::REASON_CONNECT_TIMEOUT.to_string(),
        "неведомая причина".to_string(),
    ] {
        for packaged in [false, true] {
            let shown = w::no_daemon_text_in(&reason, packaged);
            assert!(
                shown.contains(&w::text_retry_daemon()),
                "отказ «{reason}» не говорит покупателю, что делать: {shown}"
            );
            assert!(!shown.contains(&reason), "сырая причина на экране: {shown}");
            if !packaged {
                assert!(!shown.contains("пакет Rexon"), "{shown}");
            }
        }
    }
}

/// False advice removed (28.09.2026): «it is installed together with Rexon —
/// reinstall Rexon» sounded where the engine was not shipped, and
/// reinstalling gave nothing. Now «reinstall the package» — only when the
/// engine really comes with the package.
#[test]
fn missing_engine_text_is_honest_about_the_package() {
    use rexon_core::we_client as c;
    let raw = c::daemon_not_found_message();
    {
        let _lang = i18n_pin::ru();
        let off_package = w::no_daemon_text_in(&raw, false);
        let packaged = w::no_daemon_text_in(&raw, true);
        assert!(!off_package.contains("ставится вместе с Rexon"), "{off_package}");
        assert!(!off_package.contains("ереустановите"), "{off_package}");
        assert!(off_package.contains("установлена без движка"), "{off_package}");
        // Since 30.10.2026 the engine is a separate dependency package: the advice names it.
        assert!(packaged.contains("Переустановите пакет waywallen"), "{packaged}");
        assert_eq!(w::no_daemon_text(&raw), off_package, "прежняя дверь = не пакет");
        // The health state is unknown to the screen — it no longer promises «outdated».
        let not_ready = match w::health_verdict("degraded") {
            w::HealthVerdict::NotReady(r) => r,
            w::HealthVerdict::Ready => unreachable!(),
        };
        assert!(!w::no_daemon_text_in(&not_ready, true).contains("устарел"));
    }
    // The source language is English; Russian comes from po/ru.po.
    let _lang = i18n_pin::en();
    let en = w::no_daemon_text_in(&raw, true);
    assert!(en.starts_with("The waywallen wallpaper engine is missing."), "{en}");
    assert!(en.contains("Reinstall the waywallen package"), "{en}");
    let en = w::no_daemon_text_in(&raw, false);
    assert!(en.contains("installed without the engine"), "{en}");
}

// ── «Developer» page ───────────────────────────────────────────

#[test]
fn settings_keys_and_defaults_match() {
    assert_eq!(w::KEY_AUTO_DOWNLOAD, "dev_engine_auto_download");
    assert_eq!(w::KEY_AUTO_UPSTREAM, "dev_auto_upstream_check");
    assert_eq!(w::KEY_AUTO_PARITY, "dev_auto_parity_check");
    // auto-INSTALL off, auto-CHECKS on
    assert!(!w::DEFAULT_AUTO_DOWNLOAD);
    assert!(w::DEFAULT_AUTO_UPSTREAM);
    assert!(w::DEFAULT_AUTO_PARITY);
}

#[test]
fn target_status_lines_match_the_original() {
    let _lang = i18n_pin::ru();
    let initial = w::target_status_initial("open-wallpaper-engine");
    assert_eq!(initial.text, "open-wallpaper-engine: ещё не проверялось");
    assert_eq!(initial.qss, "font-size: 13px;");

    let failed = w::target_status_line(false, "waywallen", "рейт-лимит", false);
    assert_eq!(failed.text, "waywallen: рейт-лимит");
    assert_eq!(failed.qss, "font-size: 13px; color: #c62828;");

    let fresh = w::target_status_line(true, "waywallen", "0.2.6 — свежая", false);
    assert_eq!(fresh.text, "✅ waywallen: 0.2.6 — свежая");
    assert_eq!(fresh.qss, "font-size: 13px; color: #2e7d32;");

    let update = w::target_status_line(true, "waywallen", "есть 0.2.7", true);
    assert_eq!(update.text, "⬆️ waywallen: есть 0.2.7");
    assert_eq!(update.qss, "font-size: 13px; color: #e65100;");
}

#[test]
fn installed_and_seen_lines_match() {
    let _lang = i18n_pin::ru();
    let ww = w::waywallen_installed_line();
    assert_eq!(
        ww.text,
        "✅ waywallen (приложение-интерфейс): обновлён, требуется перезапуск демона"
    );
    assert_eq!(ww.qss, "font-size: 13px; color: #2e7d32;");

    assert_eq!(
        w::owe_installed_line("0.4.1").text,
        "✅ open-wallpaper-engine (движок рендеров): установлен плагин 0.4.1"
    );
    assert_eq!(
        w::owe_installed_line("").text,
        "✅ open-wallpaper-engine (движок рендеров): установлен плагин ?"
    );
    assert_eq!(
        w::owe_seen_line("open-wallpaper-engine", "v0.4.1").text,
        "✅ open-wallpaper-engine: v0.4.1 отмечена изученной"
    );
}

#[test]
fn version_label_speaks_with_one_voice() {
    let _lang = i18n_pin::ru();
    // ⚠️ Fix 3: one field — one name. In the original before the check it was
    // «Engine version: —», after it — «waywallen version: …».
    assert_eq!(w::version_label_initial(), "Версия waywallen: —");
    assert!(w::version_label_initial().starts_with("Версия waywallen: "));
    assert_eq!(w::version_label("0.2.6"), "Версия waywallen: 0.2.6");
    assert_eq!(w::version_label(""), "Версия waywallen: (неизвестна)");
    assert_eq!(w::version_label("   "), "Версия waywallen: (неизвестна)");
}

#[test]
fn confirmations_and_results_match() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::confirm_update_text("v0.2.7", "https://x/y.AppImage"),
        "Вышла новая версия движка (v0.2.7).\nСкачать и установить?\n\n\
Источник:\nhttps://x/y.AppImage"
    );
    assert_eq!(
        w::confirm_update_text("", "u"),
        "Вышла новая версия движка (?).\nСкачать и установить?\n\nИсточник:\nu"
    );
    assert_eq!(
        w::confirm_owe_text("v0.4.1", "u"),
        "Установить open-wallpaper-engine v0.4.1 как плагин waywallen?\n\nИсточник:\nu"
    );
    assert_eq!(
        w::install_done_text(true, "готово."),
        "Установлено: готово. Перезапустите демон для применения."
    );
    assert_eq!(w::install_done_text(false, "нет прав"), "Ошибка: нет прав");
    assert_eq!(
        w::owe_done_text(true, "распакован"),
        "OWE-плагин установлен: распакован"
    );
    assert_eq!(
        w::owe_done_text(false, "битый zip"),
        "Ошибка OWE-плагина: битый zip"
    );
    assert_eq!(
        w::owe_gate_refused_text("нужна waywallen ≥ 0.2.3"),
        "OWE-плагин не установлен: нужна waywallen ≥ 0.2.3"
    );
}

#[test]
fn owe_ipc_success_text_matches() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::owe_ipc_success_text("org.waywallen.open-wallpaper-engine", true),
        "плагин org.waywallen.open-wallpaper-engine установлен через демон. \
Требуется перезапуск демона."
    );
    assert_eq!(
        w::owe_ipc_success_text("org.waywallen.open-wallpaper-engine", false),
        "плагин org.waywallen.open-wallpaper-engine установлен через демон. \
Рендереры перезапущены демоном (hot-install)."
    );
}

#[test]
fn changelog_panel_falls_back_to_honest_text() {
    let _lang = i18n_pin::ru();
    assert_eq!(w::changelog_panel_text(""), "Изменений относительно нашей версии нет.");
    assert_eq!(w::changelog_panel_text("v0.2.7\n  раз"), "v0.2.7\n  раз");
}

// ── Fix 2: the IPC timeout knows its attempt ────────────────────────

#[test]
fn stale_ipc_timeout_does_not_kill_the_next_install() {
    // waiting for the answer to attempt 1, the alarm is from it too — the timeout is legitimate
    assert!(w::owe_ipc_timeout_fires(1, 1, true));
    // the answer already arrived (waiting for nothing) — stay silent
    assert!(!w::owe_ipc_timeout_fires(1, 1, false));
    // ⚠️ Fix 2: an alarm from the previous attempt, the second is running — stay
    // silent, otherwise it would delete its temporary zip and lie «the daemon did not answer»
    assert!(!w::owe_ipc_timeout_fires(1, 2, true));
}

// ── Parity check ──────────────────────────────────────────────────

#[test]
fn parity_header_and_badge_match_the_original() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::parity_report_header("0.2.6", "v0.2.6"),
        "Демон: 0.2.6 · proto-снапшот Rexon: v0.2.6\n"
    );
    assert_eq!(
        w::parity_report_header("", ""),
        "Демон: (версия неизвестна) · proto-снапшот Rexon: (тег не записан)\n"
    );

    // the protocol snapshot diverged — «✅» is not shown at all
    let b = w::parity_badge(Some(false), "0.2.9", "v0.2.6", false, 0, 0, 0);
    assert_eq!(
        b.text,
        "⚠️ демон 0.2.9 ≠ снапшот протокола v0.2.6 — нужен proto-resync"
    );
    assert_eq!(b.qss, "font-size: 13px; color: #e65100; font-weight: 600;");

    // matched and no divergences
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", false, 0, 0, 0);
    assert_eq!(b.text, "✅ схема renderer-настроек");
    assert_eq!(b.qss, "font-size: 13px; color: #2e7d32;");

    // the daemon version could not be found out — an honest caveat
    let b = w::parity_badge(None, "", "v0.2.6", false, 0, 0, 0);
    assert_eq!(
        b.text,
        "✅ схема renderer-настроек (версию демона сверить не удалось)"
    );

    // divergences: with important ones and without
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", true, 3, 0, 0);
    assert_eq!(b.text, "⚠️ расхождений: 3");
    assert_eq!(b.qss, "font-size: 13px; color: #c62828; font-weight: 600;");
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", true, 3, 2, 0);
    assert_eq!(b.text, "⚠️ расхождений: 3 (важных: 2)");

    // no divergences, but some settings had nothing to compare with — «⚠️» instead of «✅»
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", false, 0, 0, 4);
    assert_eq!(b.text, "⚠️ схема renderer-настроек · не проверено: 4");
    assert_eq!(b.qss, "font-size: 13px; color: #e65100;");
    // real divergences are not hidden by the caveat
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", true, 3, 0, 4);
    assert_eq!(b.text, "⚠️ расхождений: 3");
}

#[test]
fn drift_flag_covers_both_causes() {
    assert!(w::parity_drift_flag(true, Some(true)));
    assert!(w::parity_drift_flag(false, Some(false)));
    assert!(!w::parity_drift_flag(false, Some(true)));
    assert!(!w::parity_drift_flag(false, None));
}

// ── Workers ──────────────────────────────────────────────────────────

#[test]
fn download_progress_matches_the_original_mib_math() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::download_progress("", 1 << 20, 200 << 20),
        Some("Скачивание… 1 / 200 МиБ".to_string())
    );
    assert_eq!(
        w::download_progress("OWE-плагин: ", 5 << 20, 12 << 20),
        Some("OWE-плагин: Скачивание… 5 / 12 МиБ".to_string())
    );
    // the server did not tell the size — the original was silent, so are we
    assert_eq!(w::download_progress("", 1234, 0), None);
    // an incomplete mebibyte is rounded down, like the shift in the original
    assert_eq!(
        w::download_progress("", (1 << 20) - 1, 1 << 20),
        Some("Скачивание… 0 / 1 МиБ".to_string())
    );
}

#[test]
fn download_start_texts_match() {
    let _lang = i18n_pin::ru();
    assert_eq!(w::download_start_text(w::PREFIX_ENGINE), "Скачивание…");
    assert_eq!(
        w::download_start_text(&w::prefix_owe()),
        "OWE-плагин: скачивание…"
    );
}

#[test]
fn download_failure_names_its_reason() {
    let _lang = i18n_pin::ru();
    // The reason must reach the person: without it «failed/cancelled» cannot
    // tell a network drop from a core import error (live case of 09.09.2026).
    let o = w::after_download(w::PREFIX_ENGINE, false, false,
                              Some("ImportError: rexon_py")).unwrap();
    assert_eq!(o.message, "скачивание не удалось: ImportError: rexon_py");

    let o = w::after_download(&w::prefix_owe(), false, false,
                              Some("URLError: timed out")).unwrap();
    assert_eq!(o.message,
               "скачивание OWE-плагина не удалось: URLError: timed out");

    // An empty reason is not shown — this is a cancellation, not a failure.
    let o = w::after_download(w::PREFIX_ENGINE, false, false, Some("   ")).unwrap();
    assert_eq!(o.message, "скачивание не удалось/отменено");
}

#[test]
fn worker_steps_match_the_original_messages() {
    let _lang = i18n_pin::ru();
    // the download failed: no file — nothing to delete
    let o = w::after_download(w::PREFIX_ENGINE, false, false, None).unwrap();
    assert!(!o.ok);
    assert_eq!(o.message, "скачивание не удалось/отменено");
    assert!(!o.remove_tmp);

    let o = w::after_download(&w::prefix_owe(), false, false, None).unwrap();
    assert_eq!(o.message, "скачивание OWE-плагина не удалось/отменено");

    // quitting the app in the middle of a download: the file is there, remove it
    let o = w::after_download(w::PREFIX_ENGINE, true, true, None).unwrap();
    assert_eq!(o.message, "отменено (выход из приложения)");
    assert!(o.remove_tmp);

    // all is well — go on
    assert!(w::after_download(w::PREFIX_ENGINE, true, false, None).is_none());

    // validation
    assert!(w::after_validate(false, true, "").is_none());
    let o = w::after_validate(false, false, "не исполняемый").unwrap();
    assert_eq!(o.message, "AppImage невалиден: не исполняемый");
    assert!(o.remove_tmp);
    let o = w::after_validate(true, false, "нет plugin.toml").unwrap();
    assert_eq!(o.message, "плагин-zip невалиден: нет plugin.toml");
    assert!(o.remove_tmp);
}

#[test]
fn failed_appimage_install_no_longer_leaks_the_download() {
    let _lang = i18n_pin::ru();
    // ⚠️ Fix 1: the original removed the downloaded ~200 MiB only on cancellation
    // and on a failed validation. A failure of the install itself left the file
    // in /tmp — which is RAM on this machine.
    let bad = w::after_install_appimage(false, "Text file busy");
    assert!(!bad.ok);
    assert!(bad.remove_tmp, "провал установки обязан убрать временный файл");
    let good = w::after_install_appimage(true, "готово.");
    assert!(good.ok);
    assert!(good.remove_tmp);
}

#[test]
fn plugin_route_hands_the_zip_over_only_for_ipc() {
    let _lang = i18n_pin::ru();
    // the daemon is connected → the zip goes to the GUI thread under IPC
    // (QWebSocket must not be touched from a foreign thread), the worker itself
    // does not remove the file
    let ipc = w::after_validate_plugin_route(false, "архив в порядке");
    assert!(ipc.ok && ipc.hand_over_tmp && !ipc.remove_tmp);
    // the daemon is not running → we install ourselves, nothing to hand over
    let fs = w::after_validate_plugin_route(true, "архив в порядке");
    assert!(fs.ok && !fs.hand_over_tmp && !fs.remove_tmp);
    // after an FS install the temporary zip is removed in both outcomes
    assert!(w::after_install_plugin_fs(true, "ок").remove_tmp);
    assert!(w::after_install_plugin_fs(false, "нет прав").remove_tmp);
}

// ── GUI thread decisions ───────────────────────────────────────────────

#[test]
fn auto_install_needs_all_three_conditions() {
    assert!(w::should_auto_install(true, true, false));
    assert!(!w::should_auto_install(false, true, false));
    assert!(!w::should_auto_install(true, false, false));
    // once per session
    assert!(!w::should_auto_install(true, true, true));
}

#[test]
fn plugin_install_mode_follows_the_daemon() {
    assert!(!w::plugin_fs_install(true));
    assert!(w::plugin_fs_install(false));
}

/// The numbers of the dev page and the style sheets built from them are one
/// and the same. A guard for when one is edited and the other forgotten: the
/// page has two versions, the widget one reads the style string, the QML
/// scene the number itself.
#[test]
fn dev_page_style_numbers_match_their_stylesheets() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        w::DEV_TITLE_QSS,
        format!(
            "font-size: {}px; font-weight: {};",
            w::DEV_TITLE_FONT_SIZE,
            w::DEV_TITLE_FONT_WEIGHT
        )
    );
    assert_eq!(
        w::DEV_SECTION_QSS,
        format!(
            "font-size: {}px; font-weight: {};",
            w::DEV_SECTION_FONT_SIZE,
            w::DEV_SECTION_FONT_WEIGHT
        )
    );
    assert_eq!(
        w::DEV_MONO_QSS,
        format!("font-family: monospace; font-size: {}px;", w::DEV_MONO_FONT_SIZE)
    );
    assert_eq!(
        w::STATUS_NEUTRAL_QSS,
        format!("font-size: {}px;", w::DEV_STATUS_FONT_SIZE)
    );
    assert_eq!(
        w::muted_qss("rgba(1,2,3,0.6)", w::DEV_MUTED_FONT_SIZE),
        "font-size: 12px; color: rgba(1,2,3,0.6);"
    );
}

/// The status line and the parity badge: the paint and its style sheet come
/// from one decision. The widget version takes `Styled`, the QML scene `StatusLine`.
#[test]
fn status_line_and_styled_agree() {
    let parts = w::target_status_parts(true, "waywallen", "0.3.7", true);
    assert_eq!(parts.color, w::COLOR_WARN);
    assert!(!parts.bold);
    assert_eq!(parts.to_styled(), w::target_status_line(true, "waywallen", "0.3.7", true));

    let initial = w::target_status_initial_parts("owe");
    assert_eq!(initial.color, "");
    assert_eq!(initial.to_styled().qss, w::STATUS_NEUTRAL_QSS);

    let badge = w::parity_badge_parts(Some(false), "0.3.5", "v0.3.7", false, 0, 0, 0);
    assert_eq!(badge.color, w::COLOR_WARN);
    assert!(badge.bold);
    assert_eq!(
        badge.to_styled(),
        w::parity_badge(Some(false), "0.3.5", "v0.3.7", false, 0, 0, 0)
    );
}

// ── English interface ─────────────────────────────────────────────

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// All tab labels for a person — without Russian letters in English. Raw
/// refusal reasons (`TEXT_HEALTH_TIMEOUT`, `REASON_CONNECT_TIMEOUT`, the
/// `health` verdict) go to the log and are not translated — they are absent
/// here on purpose.
#[test]
fn english_we_window_texts_have_no_russian() {
    let _lang = i18n_pin::en();
    let mut texts: Vec<String> = vec![
        w::text_connecting(),
        w::text_no_daemon(),
        w::text_retry_daemon(),
        w::text_no_qml_modules(),
        w::dev_title(),
        w::dev_subtitle(),
        w::dialog_yes(),
        w::dialog_no(),
        w::section_upstream(),
        w::section_parity(),
        w::cb_auto_upstream(),
        w::cb_auto_download(),
        w::cb_auto_parity(),
        w::btn_check(),
        w::btn_update(),
        w::btn_owe(),
        w::btn_seen(),
        w::btn_copy(),
        w::btn_parity(),
        w::label_changelog(),
        w::placeholder_changelog(),
        w::placeholder_parity(),
        w::version_label_initial(),
        w::version_label(""),
        w::version_label("0.4.2"),
        w::target_status_initial("waywallen").text,
        w::waywallen_installed_line().text,
        w::owe_installed_line("0.4.1").text,
        w::owe_seen_line("open-wallpaper-engine", "v0.4.1").text,
        w::text_check_running(),
        w::text_check_done(),
        w::text_check_failed(),
        w::text_changelog_copied(),
        w::changelog_panel_text(""),
        w::confirm_update_text("v0.4.3", "https://x/y.AppImage"),
        w::confirm_update_title(),
        w::confirm_owe_text("v0.4.1", "u"),
        w::confirm_owe_title(),
        w::install_done_text(true, "done."),
        w::install_done_text(false, "denied"),
        w::owe_done_text(true, "unpacked"),
        w::owe_done_text(false, "broken zip"),
        w::owe_gate_refused_text("waywallen is too old"),
        w::text_owe_ipc_running(),
        w::owe_ipc_success_text("owe", true),
        w::owe_ipc_success_text("owe", false),
        w::text_owe_ipc_refused(),
        w::text_owe_ipc_timeout(),
        w::text_parity_no_daemon(),
        w::text_parity_request_failed(),
        w::parity_report_header("", ""),
        w::parity_report_header("0.4.2", "v0.4.2"),
        w::parity_badge(Some(false), "0.4.3", "v0.4.2", false, 0, 0, 0).text,
        w::parity_badge(None, "", "", false, 0, 0, 0).text,
        w::parity_badge(Some(true), "0.4.2", "v0.4.2", false, 0, 0, 0).text,
        w::parity_badge(Some(true), "0.4.2", "v0.4.2", true, 3, 0, 0).text,
        w::parity_badge(Some(true), "0.4.2", "v0.4.2", true, 3, 2, 0).text,
        w::download_progress(&w::prefix_owe(), 3 << 20, 10 << 20).unwrap(),
        w::download_progress(w::PREFIX_ENGINE, 3 << 20, 10 << 20).unwrap(),
        w::download_start_text(&w::prefix_owe()),
        w::download_start_text(w::PREFIX_ENGINE),
        w::text_validate_appimage(),
        w::text_validate_plugin(),
        w::text_install_plugin_fs(),
        w::text_cancelled_on_quit(),
    ];
    for prefix in [w::PREFIX_ENGINE.to_string(), w::prefix_owe()] {
        for reason in [None, Some("timeout")] {
            texts.push(w::after_download(&prefix, false, false, reason).unwrap().message);
        }
    }
    for is_plugin in [false, true] {
        texts.push(w::after_validate(is_plugin, false, "bad").unwrap().message);
    }
    for item in w::build_rail(true, true).items {
        texts.push(item.label);
    }
    texts.push(w::no_qml_rail_item().label);
    for text in &texts {
        assert!(!has_cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// The English sounds natural, and the rail uses waywallen's own words.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    let labels: Vec<String> = w::build_rail(true, true).items.into_iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        ["Wallpapers", "Discover", "Displays", "Status", "Plugins", "Settings", "About", "Dev"]
    );
    assert_eq!(w::version_label_initial(), "waywallen version: —");
    assert_eq!(w::version_label("0.4.2"), "waywallen version: 0.4.2");
    assert_eq!(
        w::download_progress(&w::prefix_owe(), 3 << 20, 10 << 20).as_deref(),
        Some("OWE plugin: Downloading… 3 / 10 MiB")
    );
    assert_eq!(w::download_start_text(&w::prefix_owe()), "OWE plugin: downloading…");
    assert_eq!(
        w::parity_badge(Some(true), "", "", true, 3, 2, 0).text,
        "⚠️ mismatches: 3 (2 important)"
    );
    // The button the refusal screen refers to is named there with its own words.
    use rexon_core::we_client as c;
    for reason in [c::daemon_poll_timeout_message(), "unknown".to_string()] {
        for packaged in [false, true] {
            let shown = w::no_daemon_text_in(&reason, packaged);
            assert!(shown.contains(&format!("“{}”", w::text_retry_daemon())), "{shown}");
        }
    }
}

/// «In Russian as it was»: rail and dev page labels from `po/ru.po` are
/// verbatim the former literals (snapshot before moving to the catalog).
#[test]
fn russian_reads_as_before() {
    let _lang = i18n_pin::ru();
    let labels: Vec<String> = w::build_rail(true, true).items.into_iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        ["Обои", "Обзор", "Мониторы", "Состояние", "Плагины", "Настройки", "О программе", "Разраб."]
    );
    assert_eq!(w::no_qml_rail_item().label, "Обои");
    let before = [
        (w::text_connecting(), "Запускаем движок обоев waywallen…"),
        (w::text_no_daemon(), "Движок обоев waywallen не запустился"),
        (w::text_retry_daemon(), "Повторить запуск"),
        (
            w::text_no_qml_modules(),
            // The «  » indent before commands was eaten by the `\` line continuation
            // already in the original literal — it was not on screen before either.
            "QML-модули интерфейса не собраны.\n\nСоберите их и перезапустите Rexon:\n\
bash tools/build_qml_material.sh\nbash tools/build_qml_control.sh",
        ),
        (w::dev_title(), "Разработчик"),
        (w::dev_subtitle(), "Скрытая страница (REXON_DEV=1). В Steam-сборке недоступна покупателю."),
        (w::dialog_yes(), "Да"),
        (w::dialog_no(), "Нет"),
        (w::section_upstream(), "Обновления upstream"),
        (w::section_parity(), "Отчёт о паритете UI"),
        (w::cb_auto_upstream(), "Авто-проверка обновлений upstream при запуске (сеть)"),
        (
            w::cb_auto_download(),
            "Авто-устанавливать обновления waywallen и OWE-плагина (после валидации, с .bak)",
        ),
        (w::cb_auto_parity(), "Авто-проверка паритета UI при открытии страницы"),
        (w::btn_check(), "Проверить сейчас"),
        (w::btn_update(), "Скачать и установить waywallen"),
        (w::btn_owe(), "Скачать и установить OWE-плагин"),
        (w::btn_seen(), "OWE: отметить изученным"),
        (w::btn_copy(), "Копировать всё"),
        (w::btn_parity(), "Сверить с движком"),
        (w::label_changelog(), "Что изменилось:"),
        (w::placeholder_changelog(), "Список релизов с release notes появится после проверки."),
        (w::placeholder_parity(), "Нажмите «Сверить с движком» или откройте страницу."),
        (w::text_check_running(), "Проверка upstream…"),
        (w::text_check_done(), "Проверка завершена."),
        (w::text_check_failed(), "Не удалось проверить обновления (сеть/рейт-лимит GitHub)."),
        (w::text_changelog_copied(), "Changelog скопирован в буфер."),
        (w::text_changelog_empty(), "Изменений относительно нашей версии нет."),
        (w::confirm_update_title(), "Обновление движка"),
        (w::confirm_owe_title(), "Установка OWE-плагина"),
        (w::text_owe_ipc_running(), "OWE-плагин: установка через демон…"),
        (w::text_owe_ipc_refused(), "демон отверг установку плагина"),
        (
            w::text_owe_ipc_timeout(),
            "демон не ответил на установку плагина за 60 секунд — проверьте его журнал и повторите",
        ),
        (w::text_parity_no_daemon(), "Демон не подключён."),
        (w::text_parity_request_failed(), "Ошибка запроса схемы у движка"),
        (w::prefix_owe(), "OWE-плагин: "),
        (w::text_validate_appimage(), "Проверка нового AppImage…"),
        (w::text_validate_plugin(), "OWE-плагин: проверка архива…"),
        (w::text_install_plugin_fs(), "OWE-плагин: установка (демон не запущен)…"),
        (w::text_cancelled_on_quit(), "отменено (выход из приложения)"),
    ];
    for (got, want) in before {
        assert_eq!(got, want);
    }
}

// ── Vendor interface colours ──────────────────────────────────────

/// By default the tab is painted with the Rexon skin: the standard «Dark
/// (Steam)» gives exactly the page paints of this skin.
#[test]
fn vendor_theme_follows_rexon_skin_by_default() {
    let t = w::vendor_theme(rexon_core::ui_base::DEFAULT_SKIN);
    assert!(!t.follow_vendor);
    assert_eq!(t.accent, "#66c0f4");
    assert_eq!(t.background, "#1b2838");
    assert_eq!(t.text, "#ffffff");
    assert!(t.dark);
    // An unknown skin falls back to the standard one, not to the waywallen scheme.
    assert_eq!(w::vendor_theme("no-such-skin"), t);
}

/// Every Rexon skin except «As in waywallen» paints the tab with its own
/// paints: background and text are the same as on neighbouring QML pages,
/// darkness is by background, text is legible on the background.
#[test]
fn vendor_theme_takes_colors_of_every_rexon_skin() {
    use rexon_core::ui_base as ui;
    for (id, _) in ui::available_skins() {
        let t = w::vendor_theme(id);
        let palette = ui::qml_palette(id);
        assert_eq!(t.follow_vendor, id == ui::VENDOR_SCHEME_SKIN, "{id}");
        assert_eq!(&t.background, &palette["surface_page"], "{id}");
        assert_eq!(&t.accent, &palette["accent"], "{id}");
        assert_eq!(&t.text, &palette["text_primary"], "{id}");
        assert_ne!(t.text, t.background, "{id}: текст слился с фоном");
        let (r, g, b) = ui::parse_hex(&t.background).unwrap();
        assert_eq!(t.dark, ui::is_dark_rgb(r, g, b), "{id}");
    }
    let light = w::vendor_theme("light");
    assert!(!light.dark && !light.follow_vendor);
    assert_eq!(light.accent, "#2563eb");
}

/// waywallen's own scheme is a separate item in the skin list, and only it.
#[test]
fn vendor_scheme_is_a_separate_skin() {
    use rexon_core::ui_base as ui;
    assert!(ui::is_valid_skin(ui::VENDOR_SCHEME_SKIN));
    assert_ne!(ui::DEFAULT_SKIN, ui::VENDOR_SCHEME_SKIN);
    let t = w::vendor_theme(ui::VENDOR_SCHEME_SKIN);
    assert!(t.follow_vendor);
    // The page's start screens are painted with dark Material paints.
    assert!(t.dark);
    assert_eq!(t.background, "#201f21");
}

/// Material scheme roles are painted by the skin one by one: on «Dark
/// (Steam)» the tab cards (`surface_container`) are exactly the page
/// background `#1b2838`, the work area the field paint, the text the skin text.
#[test]
fn vendor_roles_paint_cards_with_skin_colors() {
    use rexon_core::ui_base as ui;
    let t = w::vendor_theme(ui::DEFAULT_SKIN);
    assert_eq!(t.roles["surface_container"], "#1b2838");
    assert_eq!(t.roles["surface"], "#16202d");
    assert_eq!(t.roles["background"], "#16202d");
    assert_eq!(t.roles["on_surface"], "#ffffff");
    assert_eq!(t.roles.len(), w::VENDOR_SCHEME_ROLES.len());
    let json: serde_json::Value = serde_json::from_str(&w::vendor_roles_json(&t)).unwrap();
    assert_eq!(json["surface_container"], "#1b2838");
}

/// For every Rexon skin each role is given a ready `#rrggbb` paint from its
/// own palette; waywallen's own scheme has no roles — its own settings apply there.
#[test]
fn vendor_roles_cover_every_rexon_skin() {
    use rexon_core::ui_base as ui;
    for (id, _) in ui::available_skins() {
        let t = w::vendor_theme(id);
        if t.follow_vendor {
            assert!(t.roles.is_empty(), "{id}");
            assert_eq!(w::vendor_roles_json(&t), "{}");
            continue;
        }
        let palette = ui::qml_palette(id);
        for (role, key) in w::VENDOR_SCHEME_ROLES {
            let got = t.roles.get(*role).unwrap_or_else(|| panic!("{id}: нет роли {role}"));
            assert!(ui::parse_hex(got).is_some(), "{id}: {role} = {got}");
            assert_eq!(got, &palette[*key], "{id}: {role}");
        }
        assert_ne!(t.roles["surface_container"], t.roles["on_surface"], "{id}: текст слился с фоном");
    }
}
