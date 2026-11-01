//! Сверка [`rexon_core::we_window`] — вкладки Wallpaper Engine.
//!
//! Ожидаемые значения выписаны ДОСЛОВНО (когда-то — из прежней
//! `rexon/qt_wallpaper_engine/window.py`, коммит `ec8a809`; с 02.10.2026
//! Python в Rexon — только архив, и эти записи — собственный эталон Rust),
//! включая склейку строк, разорванных по ширине колонки. Отдельно проверены три починки: удаление временного
//! файла после провалившейся установки, опознание попытки IPC-таймаутом и
//! единая подпись версии движка.

use rexon_core::we_window as w;

mod i18n_pin;

// ── Скрытый режим разработчика ───────────────────────────────────────

#[test]
fn dev_env_beats_marker_both_ways() {
    let _lang = i18n_pin::ru();
    // env выключает даже при маркере («как увидит покупатель»)
    for raw in ["0", "false", "no", "NO", " False "] {
        assert!(!w::dev_mode(raw, true), "{raw}");
        assert!(!w::dev_mode(raw, false), "{raw}");
    }
    // env включает без маркера
    for raw in ["1", "true", "yes", "YES", " True "] {
        assert!(w::dev_mode(raw, false), "{raw}");
        assert!(w::dev_mode(raw, true), "{raw}");
    }
    // пусто/мусор — решает маркер
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

// ── Рейл и страницы ──────────────────────────────────────────────────

#[test]
fn qml_specs_match_the_vendor_pages() {
    let _lang = i18n_pin::ru();
    let specs = w::qml_page_specs();
    // Пятым в строке — признак вендорной ШАПКИ. Он тут не для полноты:
    // страница без шапки теряет заголовок и ВСЕ кнопки действий, а список
    // при этом рисуется, и снимок выглядит рабочим. Само ПРАВИЛО, по
    // которому признак ставится, сторожит `we_rail_vendor_parity.rs` —
    // он читает саму вендорную страницу, а не этот список.
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
    // QML собран, без dev: семь пунктов, семь контейнеров
    let p = w::build_rail(true, false);
    assert_eq!(p.items.len(), 7);
    assert_eq!(p.stub_index, None);
    assert_eq!(p.dev_index, None);
    for i in 0..7 {
        assert!(p.qml_for_index(i).is_some(), "{i}");
        assert!(p.vendor_qml_for_index(i).is_some(), "{i}");
    }

    // QML собран, dev: восьмым пунктом «Разраб.», у него вендорной страницы нет
    let p = w::build_rail(true, true);
    assert_eq!(p.items.len(), 8);
    assert_eq!(p.dev_index, Some(7));
    assert_eq!(p.items[7].id, "dev");
    // Страница «Разработчик» — НАША (QML-модуль `org.rexon.gui`): имя файла
    // у пункта есть, но вендорной страницы у него нет и быть не может.
    assert_eq!(p.qml_for_index(7), Some(w::DEV_PAGE_QML));
    assert_eq!(p.vendor_qml_for_index(7), None);
    assert!(!p.items[7].vendor);

    // QML не собран: заглушка нулевой страницей
    let p = w::build_rail(false, false);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].id, "no_qml");
    assert_eq!(p.stub_index, Some(0));
    assert_eq!(p.qml_for_index(0), None);
    assert_eq!(p.vendor_qml_for_index(0), None);

    // QML не собран + dev: заглушка и dev идут подряд, индексы не разъезжаются
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

// ── Цвета и стили ────────────────────────────────────────────────────

#[test]
fn variant_color_matches_the_original_format() {
    assert_eq!(w::variant_color(0, 0, 0), "rgba(0,0,0,0.6)");
    assert_eq!(w::variant_color(255, 255, 255), "rgba(255,255,255,0.6)");
    assert_eq!(w::variant_color(31, 35, 38), "rgba(31,35,38,0.6)");
}

#[test]
fn rail_qss_matches_the_original_concatenation() {
    // ⭐ Скругление здесь НАМЕРЕННО разошлось со снапшотом оригинала
    // (`ec8a809` давал 12px): 12 были нашей выдумкой, а вкладка чужая, и
    // мерило у неё одно — waywallen. Его рейл рисует подсветку выбранного
    // «пилюлей», `radius: height / 2` (`RailItem.qml` модуля
    // `Qcm.Material`), поэтому `RAIL_BUTTON_RADIUS` теперь считается от
    // высоты пункта: 56 / 2 = 28. Остальная склейка — дословно прежняя.
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
    // Долю таблицы стилей парсер Qt усекает: 0.03 → 7/255, 0.02 → 5/255.
    // QML-версия берёт ту же краску восьмибитной, иначе полоса рейла
    // выходит на уровень светлее оригинала (замер 04.09.2026).
    assert_eq!(w::rail_background_argb_hex(true), "#07ffffff");
    assert_eq!(w::rail_background_argb_hex(false), "#05000000");
    assert_eq!((w::RAIL_BG_ALPHA_DARK * 255.0) as u8, 7);
    assert_eq!((w::RAIL_BG_ALPHA_LIGHT * 255.0) as u8, 5);
    // И сами доли — те, что стоят в таблице стилей оригинала.
    assert!(w::rail_qss(true, 1, 2, 3).contains("rgba(255,255,255,0.03)"));
    assert!(w::rail_qss(false, 1, 2, 3).contains("rgba(0,0,0,0.02)"));
}

#[test]
fn rail_selected_color_matches_the_qss_alpha() {
    let _lang = i18n_pin::ru();
    // Ту же долю подсветки выбранного пункта парсер Qt усекает:
    // 0.25 → 63/255. QML-версия берёт краску восьмибитной, иначе
    // подсветка выходит на уровень светлее оригинала — (51,82,107)
    // против (50,82,106) (замер 04.09.2026, раздел «Дисплеи»).
    assert_eq!(w::rail_selected_argb_hex(0x66, 0xc0, 0xf4), "#3f66c0f4");
    assert_eq!(w::rail_selected_argb_hex(37, 99, 235), "#3f2563eb");
    assert_eq!((w::RAIL_SELECTED_ALPHA * 255.0) as u8, 0x3f);
    // И сама доля — та, что стоит в таблице стилей оригинала.
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
    // Те же числа берёт QML-версия свойствами модели.
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

// ── Состояния вкладки ────────────────────────────────────────────────

#[test]
fn start_screen_qss_matches_the_original_literals() {
    let _lang = i18n_pin::ru();
    // `ic.setStyleSheet("font-size: 48px;")` и
    // `self._no_daemon_lbl.setStyleSheet("font-size: 15px;")` оригинала.
    assert_eq!(w::no_daemon_icon_qss(), "font-size: 48px;");
    assert_eq!(w::no_daemon_text_qss(), "font-size: 15px;");
    assert_eq!(w::STATE_TEXT_FONT_SIZE, 15);
    assert_eq!(w::NO_DAEMON_ICON_FONT_SIZE, 48);
    // Умолчание `QVBoxLayout` под стилем продукта, замер 03.09.2026.
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

/// Общий срок ожидания движка обязан быть КОНЕЧНЫМ и ПОСЛЕДНИМ.
///
/// Последним — потому что до него срабатывают точные сторожа: 12 секунд
/// опроса шины и 5 секунд на ответ `health`. Сделай его короче их суммы —
/// и покупатель вместо «движок запускается слишком долго» получит общие
/// слова, а мы потеряем диагноз. Проверка стоит здесь, чтобы это выяснялось
/// на `cargo test`, а не на живом окне.
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
    // Замер холодного старта демона на машине владельца — 3,2–5,0 с
    // (стенд `measure-start.sh`, 18.09.2026). Полминуты ожидания покупателю
    // уже не объяснить ничем.
    assert!(w::CONNECT_TIMEOUT_MS <= 30_000);
}

#[test]
fn health_verdict_trusts_old_daemons_and_stops_broken_ones() {
    let _lang = i18n_pin::ru();
    assert_eq!(w::health_verdict("healthy"), w::HealthVerdict::Ready);
    assert_eq!(w::health_verdict("HEALTHY"), w::HealthVerdict::Ready);
    // старый демон без поля health — пустая строка, страницы показываем
    assert_eq!(w::health_verdict(""), w::HealthVerdict::Ready);
    assert_eq!(
        w::health_verdict("degraded"),
        w::HealthVerdict::NotReady(
            "движок ответил, но не готов (состояние: degraded)".to_string()
        )
    );
}

/// Каждая причина, которую продукт умеет сказать, обязана ОПОЗНАВАТЬСЯ.
///
/// Иначе покупатель молча съезжает на общие слова: разбор идёт по строке,
/// и любое переписывание сообщения в `we_client` рвёт его беззвучно — ни
/// сборка, ни работа об этом не скажут. Поэтому сообщения берутся прямо у
/// их авторов, а не переписаны сюда копиями.
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

/// Экран отказа — для покупателя, а машинная кухня — для журнала.
///
/// Сторож ровно на то, из-за чего задача и заводилась: пути, `PATH` и слово
/// AppImage стояли на весь экран у человека, который купил программу ради
/// обоев.
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
    // Та же подробность обязана оказаться в журнале — целиком и дословно.
    let line = w::daemon_failure_log_line(&raw).expect("строка журнала");
    assert!(line.contains(&raw), "{line}");
    assert_eq!(w::daemon_failure_log_line("   "), None);

    // Каждый случай говорит, ЧТО делать: либо кнопкой, либо переустановкой
    // пакета — и совет про пакет звучит ТОЛЬКО у пакетной установки.
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

/// Ложный совет снят (28.09.2026): «он ставится вместе с Rexon —
/// переустановите Rexon» звучал там, где движка в поставке не было, и
/// переустановка ничего не давала. Теперь «переустановите пакет» —
/// только когда движок действительно приезжает с пакетом.
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
        // С 30.10.2026 движок — отдельный пакет-зависимость: совет называет его.
        assert!(packaged.contains("Переустановите пакет waywallen"), "{packaged}");
        assert_eq!(w::no_daemon_text(&raw), off_package, "прежняя дверь = не пакет");
        // Состояние health экрану неизвестно — «устарел» он больше не обещает.
        let not_ready = match w::health_verdict("degraded") {
            w::HealthVerdict::NotReady(r) => r,
            w::HealthVerdict::Ready => unreachable!(),
        };
        assert!(!w::no_daemon_text_in(&not_ready, true).contains("устарел"));
    }
    // Исходный язык — английский; русский приходит из po/ru.po.
    let _lang = i18n_pin::en();
    let en = w::no_daemon_text_in(&raw, true);
    assert!(en.starts_with("The waywallen wallpaper engine is missing."), "{en}");
    assert!(en.contains("Reinstall the waywallen package"), "{en}");
    let en = w::no_daemon_text_in(&raw, false);
    assert!(en.contains("installed without the engine"), "{en}");
}

// ── Страница «Разработчик» ───────────────────────────────────────────

#[test]
fn settings_keys_and_defaults_match() {
    assert_eq!(w::KEY_AUTO_DOWNLOAD, "dev_engine_auto_download");
    assert_eq!(w::KEY_AUTO_UPSTREAM, "dev_auto_upstream_check");
    assert_eq!(w::KEY_AUTO_PARITY, "dev_auto_parity_check");
    // авто-УСТАНОВКА выключена, авто-ПРОВЕРКИ включены
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
    // ⚠️ Починка 3: одно поле — одно имя. В оригинале до проверки было
    // «Версия движка: —», после — «Версия waywallen: …».
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

// ── Починка 2: таймаут IPC знает свою попытку ────────────────────────

#[test]
fn stale_ipc_timeout_does_not_kill_the_next_install() {
    // ждём ответа на попытку 1, будильник от неё же — таймаут законен
    assert!(w::owe_ipc_timeout_fires(1, 1, true));
    // ответ уже пришёл (ничего не ждём) — молчим
    assert!(!w::owe_ipc_timeout_fires(1, 1, false));
    // ⚠️ Починка 2: будильник от прошлой попытки, идёт вторая — молчим,
    // иначе он удалил бы её временный zip и соврал «демон не ответил»
    assert!(!w::owe_ipc_timeout_fires(1, 2, true));
}

// ── Сверка паритета ──────────────────────────────────────────────────

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

    // снапшот протокола разошёлся — «✅» не показываем вовсе
    let b = w::parity_badge(Some(false), "0.2.9", "v0.2.6", false, 0, 0, 0);
    assert_eq!(
        b.text,
        "⚠️ демон 0.2.9 ≠ снапшот протокола v0.2.6 — нужен proto-resync"
    );
    assert_eq!(b.qss, "font-size: 13px; color: #e65100; font-weight: 600;");

    // сошлись и расхождений нет
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", false, 0, 0, 0);
    assert_eq!(b.text, "✅ схема renderer-настроек");
    assert_eq!(b.qss, "font-size: 13px; color: #2e7d32;");

    // версию демона узнать не удалось — честная оговорка
    let b = w::parity_badge(None, "", "v0.2.6", false, 0, 0, 0);
    assert_eq!(
        b.text,
        "✅ схема renderer-настроек (версию демона сверить не удалось)"
    );

    // расхождения: с важными и без
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", true, 3, 0, 0);
    assert_eq!(b.text, "⚠️ расхождений: 3");
    assert_eq!(b.qss, "font-size: 13px; color: #c62828; font-weight: 600;");
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", true, 3, 2, 0);
    assert_eq!(b.text, "⚠️ расхождений: 3 (важных: 2)");

    // расхождений нет, но часть настроек сверить было не с чем — «⚠️» вместо «✅»
    let b = w::parity_badge(Some(true), "0.2.6", "v0.2.6", false, 0, 0, 4);
    assert_eq!(b.text, "⚠️ схема renderer-настроек · не проверено: 4");
    assert_eq!(b.qss, "font-size: 13px; color: #e65100;");
    // настоящие расхождения оговоркой не заслоняются
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

// ── Воркеры ──────────────────────────────────────────────────────────

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
    // размера сервер не сказал — оригинал молчал, молчим и мы
    assert_eq!(w::download_progress("", 1234, 0), None);
    // неполный мебибайт округляется вниз, как сдвиг в оригинале
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
    // Причина обязана доезжать до человека: без неё «не удалось/отменено»
    // не отличить обрыв сети от ошибки импорта ядра (живой случай 09.09.2026).
    let o = w::after_download(w::PREFIX_ENGINE, false, false,
                              Some("ImportError: rexon_py")).unwrap();
    assert_eq!(o.message, "скачивание не удалось: ImportError: rexon_py");

    let o = w::after_download(&w::prefix_owe(), false, false,
                              Some("URLError: timed out")).unwrap();
    assert_eq!(o.message,
               "скачивание OWE-плагина не удалось: URLError: timed out");

    // Пустую причину не показываем — это отмена, а не сбой.
    let o = w::after_download(w::PREFIX_ENGINE, false, false, Some("   ")).unwrap();
    assert_eq!(o.message, "скачивание не удалось/отменено");
}

#[test]
fn worker_steps_match_the_original_messages() {
    let _lang = i18n_pin::ru();
    // скачивание не удалось: файла нет — удалять нечего
    let o = w::after_download(w::PREFIX_ENGINE, false, false, None).unwrap();
    assert!(!o.ok);
    assert_eq!(o.message, "скачивание не удалось/отменено");
    assert!(!o.remove_tmp);

    let o = w::after_download(&w::prefix_owe(), false, false, None).unwrap();
    assert_eq!(o.message, "скачивание OWE-плагина не удалось/отменено");

    // выход из приложения посреди скачивания: файл есть, убрать
    let o = w::after_download(w::PREFIX_ENGINE, true, true, None).unwrap();
    assert_eq!(o.message, "отменено (выход из приложения)");
    assert!(o.remove_tmp);

    // всё хорошо — идём дальше
    assert!(w::after_download(w::PREFIX_ENGINE, true, false, None).is_none());

    // валидация
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
    // ⚠️ Починка 1: оригинал убирал скачанные ~200 МиБ только при отмене и
    // провале валидации. Провал самой установки оставлял файл в /tmp — на
    // этой машине это ОЗУ.
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
    // демон подключён → zip уходит GUI-потоку под IPC (QWebSocket из чужого
    // потока трогать нельзя), сам воркер файл не убирает
    let ipc = w::after_validate_plugin_route(false, "архив в порядке");
    assert!(ipc.ok && ipc.hand_over_tmp && !ipc.remove_tmp);
    // демон не запущен → ставим сами, отдавать нечего
    let fs = w::after_validate_plugin_route(true, "архив в порядке");
    assert!(fs.ok && !fs.hand_over_tmp && !fs.remove_tmp);
    // после ФС-установки временный zip убирается в обоих исходах
    assert!(w::after_install_plugin_fs(true, "ок").remove_tmp);
    assert!(w::after_install_plugin_fs(false, "нет прав").remove_tmp);
}

// ── Решения GUI-потока ───────────────────────────────────────────────

#[test]
fn auto_install_needs_all_three_conditions() {
    assert!(w::should_auto_install(true, true, false));
    assert!(!w::should_auto_install(false, true, false));
    assert!(!w::should_auto_install(true, false, false));
    // один раз за сессию
    assert!(!w::should_auto_install(true, true, true));
}

#[test]
fn plugin_install_mode_follows_the_daemon() {
    assert!(!w::plugin_fs_install(true));
    assert!(w::plugin_fs_install(false));
}

/// Числа dev-страницы и таблицы стилей, которые из них собраны, — одно и то
/// же. Сторож на случай, когда правят одно и забывают второе: у страницы
/// две версии, виджетная читает строку стиля, сцена QML — само число.
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

/// Строка статуса и бейдж паритета: краска и её таблица стилей — из одного
/// решения. Виджетная версия берёт `Styled`, сцена QML — `StatusLine`.
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

// ── Английский интерфейс ─────────────────────────────────────────────

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все подписи вкладки для человека — без русских букв на английском. Сырые
/// причины отказа (`TEXT_HEALTH_TIMEOUT`, `REASON_CONNECT_TIMEOUT`, вердикт
/// `health`) идут в журнал и не переводятся — их здесь нет намеренно.
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

/// Английский звучит естественно, а рейл — словами самого waywallen.
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
    // Кнопка, на которую ссылается экран отказа, названа там её же словами.
    use rexon_core::we_client as c;
    for reason in [c::daemon_poll_timeout_message(), "unknown".to_string()] {
        for packaged in [false, true] {
            let shown = w::no_daemon_text_in(&reason, packaged);
            assert!(shown.contains(&format!("“{}”", w::text_retry_daemon())), "{shown}");
        }
    }
}

/// «По-русски как было»: подписи рейла и dev-страницы из `po/ru.po` —
/// дословно прежние литералы (снимок до перевода на каталог).
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
            // Отступ «  » перед командами съедало продолжение строки `\` ещё
            // в исходном литерале — на экране его не было и прежде.
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

// ── Цвета вендорного интерфейса ──────────────────────────────────────

/// По умолчанию вкладка красится скином Rexon: стандартный «Тёмный
/// (Steam)» даёт ровно краски страницы этого скина.
#[test]
fn vendor_theme_follows_rexon_skin_by_default() {
    let t = w::vendor_theme(rexon_core::ui_base::DEFAULT_SKIN);
    assert!(!t.follow_vendor);
    assert_eq!(t.accent, "#66c0f4");
    assert_eq!(t.background, "#1b2838");
    assert_eq!(t.text, "#ffffff");
    assert!(t.dark);
    // Незнакомый скин откатывается к стандартному, а не к схеме waywallen.
    assert_eq!(w::vendor_theme("no-such-skin"), t);
}

/// Каждый скин Rexon, кроме «Как в waywallen», красит вкладку своими
/// красками: фон и текст — те же, что у соседних QML-страниц, тёмность —
/// по фону, текст на фоне различим.
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

/// Схема самого waywallen — отдельный пункт в списке скинов, и только он.
#[test]
fn vendor_scheme_is_a_separate_skin() {
    use rexon_core::ui_base as ui;
    assert!(ui::is_valid_skin(ui::VENDOR_SCHEME_SKIN));
    assert_ne!(ui::DEFAULT_SKIN, ui::VENDOR_SCHEME_SKIN);
    let t = w::vendor_theme(ui::VENDOR_SCHEME_SKIN);
    assert!(t.follow_vendor);
    // Стартовые экраны страницы красятся тёмными красками Material.
    assert!(t.dark);
    assert_eq!(t.background, "#201f21");
}

/// Роли схемы Material красятся скином поштучно: на «Тёмном (Steam)»
/// карточки вкладки (`surface_container`) — ровно фон страницы `#1b2838`,
/// рабочая область — краска поля, текст — текст скина.
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

/// У каждого скина Rexon каждая роль задана готовой краской `#rrggbb` из
/// его же палитры; у схемы самого waywallen ролей нет — там действуют его
/// собственные настройки.
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
