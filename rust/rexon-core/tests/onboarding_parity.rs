//! Tests of the first-run tutorial scenario.
//!
//! There is no Python original — the module was written in Rust right away, so this is not a
//! golden check but a check of the rules themselves: the order of steps, "show once",
//! the difference between a first run and a deliberate refusal, stability of
//! identifiers.

use rexon_core::i18n;
use rexon_core::onboarding as ob;

/// Texts are compared in the SOURCE language: otherwise the answer would depend on the machine's
/// locale and on whether the package with catalogs is installed. Russian
/// is checked separately, against the catalog itself — `russian_catalog_keeps_old_texts`.
fn english() {
    i18n::set_language(Some(i18n::SOURCE_LANGUAGE));
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

/// Step identifiers are part of the users' saved state.
/// Renaming would make an already completed lesson show again, so
/// the list is pinned by a test on purpose.
#[test]
fn step_ids_are_stable() {
    let got: Vec<&str> = ob::STEPS.iter().map(|s| s.id).collect();
    assert_eq!(
        got,
        vec![
            "welcome",
            "tab_appearance",
            "appearance_catalog",
            "tab_wallpaper_engine",
            "tab_desktops",
            "settings_protection",
            "tab_steam",
            "finish",
        ]
    );
    assert_eq!(ob::total_steps(), 8);
}

/// Every step is bound to an existing tab of the main window or to the window
/// as a whole. A typo in a tab identifier would mean a hint
/// pointing into nowhere.
#[test]
fn anchors_point_to_real_tabs() {
    const TABS: &[&str] = &[
        "desktops",
        "appearance",
        "extensions",
        "wallpaper_engine",
        "steam",
        "settings",
    ];
    for step in ob::STEPS {
        if let Some(tab) = step.anchor.tab_id() {
            assert!(TABS.contains(&tab), "шаг {}: нет вкладки {tab}", step.id);
        }
    }
    // The first and last steps are centered cards, without binding.
    assert_eq!(ob::STEPS[0].anchor, ob::Anchor::Window);
    assert_eq!(ob::STEPS[ob::STEPS.len() - 1].anchor, ob::Anchor::Window);
}

#[test]
fn texts_are_filled() {
    for step in ob::STEPS {
        assert!(!step.title.trim().is_empty(), "шаг {}: пустой заголовок", step.id);
        assert!(step.text.len() > 40, "шаг {}: текст слишком короткий", step.id);
        // Jargon is not allowed in the tutorial — it is exactly for those who do not know it.
        for word in ["QSS", "kpackage", "layer-shell", "mmap", "D-Bus"] {
            assert!(
                !step.text.contains(word),
                "шаг {}: жаргон «{word}» в тексте для пользователя",
                step.id
            );
        }
    }
}

/// A first run differs from a deliberate refusal: otherwise a person who
/// once said «not needed» would get the question every time.
#[test]
fn start_distinguishes_first_run_from_refusal() {
    assert_eq!(ob::on_start(None, &[]), ob::Start::Ask);
    assert_eq!(ob::on_start(Some(false), &[]), ob::Start::Nothing);
    assert_eq!(ob::on_start(Some(true), &[]), ob::Start::Show("welcome"));
    // The tutorial is on but partially completed — we continue from where it stopped.
    assert_eq!(
        ob::on_start(Some(true), &ids(&["welcome", "tab_appearance"])),
        ob::Start::Show("appearance_catalog")
    );
    // Completed in full — we do not get in the way.
    assert_eq!(ob::on_start(Some(true), &ob::skip_all()), ob::Start::Nothing);
}

#[test]
fn each_step_is_shown_once() {
    let mut seen: Vec<String> = Vec::new();
    let mut order = Vec::new();
    while let Some(step) = ob::next_step(&seen) {
        order.push(step.id);
        ob::mark_seen(&mut seen, step.id);
        assert!(order.len() <= ob::total_steps() + 1, "сценарий зациклился");
    }
    assert_eq!(order.len(), ob::total_steps());
    assert!(ob::is_finished(&seen));

    // A repeated mark does not multiply entries.
    let before = seen.len();
    ob::mark_seen(&mut seen, "welcome");
    assert_eq!(seen.len(), before);
}

/// «Next» moves forward through the scenario, rather than going back to a step skipped
/// earlier — otherwise the tutorial would go in circles.
#[test]
fn next_button_moves_forward() {
    let seen = ids(&["tab_appearance"]);
    assert_eq!(
        ob::step_after("welcome", &seen).map(|s| s.id),
        Some("appearance_catalog"),
        "уже показанный шаг должен пропускаться"
    );
    // There is nowhere to go from the last step.
    assert!(ob::step_after("finish", &[]).is_none());
    // An unknown identifier (state from an old version) does not bring down
    // the tutorial — we start from the first one not shown.
    assert_eq!(
        ob::step_after("шаг-из-будущего", &[]).map(|s| s.id),
        Some("welcome")
    );
}

#[test]
fn skip_and_reset() {
    let all = ob::skip_all();
    assert!(ob::is_finished(&all), "пропуск обязан завершать сценарий");
    assert_eq!(all.len(), ob::total_steps());

    let fresh = ob::reset();
    assert!(fresh.is_empty());
    assert_eq!(ob::on_start(Some(true), &fresh), ob::Start::Show("welcome"));
}

#[test]
fn progress_label() {
    assert_eq!(ob::step_position("welcome"), Some(1));
    assert_eq!(ob::step_position("finish"), Some(ob::total_steps()));
    assert_eq!(ob::step_position("нет-такого"), None);
    english();
    assert_eq!(ob::progress_label(3, 8), "Step 3 of 8");
}

// ── Overlay: geometry, card, hover ─────────────────────────────────────────
//
// The reference is the code of `_Spotlight`/`Tour._show`/`Tour._aim` as of the day of the move.
// ⛔ Qt does not start: everything here is pure arithmetic and the step table.

#[test]
fn spotlight_hole_pads_the_target() {
    // The cutout is wider than the target by a margin on each side.
    assert_eq!(
        ob::spotlight_hole(Some((100, 50, 200, 30))),
        Some((100 - 8, 50 - 8, 200 + 16, 30 + 16))
    );
    assert_eq!(ob::TARGET_PAD, 8);
    // There is no target, or it is degenerate — we darken the whole window.
    assert_eq!(ob::spotlight_hole(None), None);
    assert_eq!(ob::spotlight_hole(Some((10, 10, 0, 20))), None);
    assert_eq!(ob::spotlight_hole(Some((10, 10, 20, -1))), None);
}

#[test]
fn card_goes_below_the_hole_when_it_fits() {
    // A 1000×700 window, the cutout at the top — the card below it with a gap.
    let p = ob::card_placement((1000, 700), (400, 200), Some((100, 40, 200, 60)));
    assert_eq!(p.y, 40 + 60 - 1 + ob::CARD_GAP);
    // Horizontally — centered on the cutout, but not closer than the gap to the window edge:
    // here the center would give exactly 0, and the card would press against the frame itself.
    assert_eq!(p.x, ob::CARD_GAP);
    // The cutout in the middle of the window — centering is visible without clamping.
    let p = ob::card_placement((1000, 700), (400, 200), Some((400, 40, 200, 60)));
    assert_eq!(p.x, (400 + 200 / 2) - 400 / 2);
}

#[test]
fn card_goes_above_when_there_is_no_room_below() {
    // The cutout at the very bottom: the card does not fit below it.
    let p = ob::card_placement((1000, 700), (400, 200), Some((100, 600, 200, 60)));
    assert_eq!(p.y, 600 - ob::CARD_GAP - 200);
    assert!(p.y >= 0);
}

#[test]
fn card_centers_when_it_fits_nowhere() {
    // A full-screen cutout — no room either above or below.
    let p = ob::card_placement((1000, 700), (400, 600), Some((0, 0, 1000, 700)));
    assert_eq!(p.y, (700 - 600) / 2);
    // Without a cutout the card is exactly centered.
    let p = ob::card_placement((1000, 700), (400, 200), None);
    assert_eq!((p.x, p.y), ((1000 - 400) / 2, (700 - 200) / 2));
}

#[test]
fn card_never_hangs_off_the_window_edge() {
    // A cutout at the left edge — the card is pressed in with a gap rather than going
    // beyond the window border.
    let p = ob::card_placement((1000, 700), (400, 200), Some((0, 40, 40, 40)));
    assert_eq!(p.x, ob::CARD_GAP);
    // The same case on the right.
    let p = ob::card_placement((1000, 700), (400, 200), Some((960, 40, 40, 40)));
    assert_eq!(p.x, 1000 - 400 - ob::CARD_GAP);
    // The card is wider than the window: nowhere to press it — but we do not move it past the left edge.
    let p = ob::card_placement((300, 700), (400, 200), Some((0, 40, 40, 40)));
    assert_eq!(p.x, ob::CARD_GAP);
}

#[test]
fn card_content_switches_buttons_on_the_last_step() {
    english();
    let first = ob::card_content("welcome").unwrap();
    assert_eq!(first.progress, "Step 1 of 8");
    assert_eq!(first.next_text, ob::btn_next());
    assert_eq!(first.skip_text, ob::btn_skip());
    assert!(first.skip_visible);

    // On the last step there is nothing left to refuse (rule #13).
    let last = ob::card_content("finish").unwrap();
    assert_eq!(last.progress, format!("Step {} of {}", ob::total_steps(), ob::total_steps()));
    assert_eq!(last.next_text, ob::btn_done());
    assert_eq!(last.skip_text, "");
    assert!(!last.skip_visible);

    // An unknown step — there must be no empty card.
    assert!(ob::card_content("нет-такого").is_none());
}

#[test]
fn every_step_has_content_and_an_aim() {
    for step in ob::STEPS {
        let c = ob::card_content(step.id).unwrap_or_else(|| panic!("нет карточки {}", step.id));
        assert!(!c.title.is_empty(), "{}: пустой заголовок", step.id);
        assert!(!c.text.is_empty(), "{}: пустой текст", step.id);
        assert!(!c.progress.is_empty(), "{}: пустая подпись", step.id);
        // Hover agrees with the step's binding.
        let aim = ob::aim_target(step.id);
        match step.anchor {
            ob::Anchor::Tab(id) => assert_eq!(aim, ob::AimTarget::TabHeader(id)),
            ob::Anchor::TabContent(id) => assert_eq!(aim, ob::AimTarget::TabContent(id)),
            ob::Anchor::Window => assert_eq!(aim, ob::AimTarget::WholeWindow),
        }
    }
    // An unknown step does not draw an empty frame.
    assert_eq!(ob::aim_target("нет-такого"), ob::AimTarget::WholeWindow);
}

#[test]
fn seen_from_settings_survives_garbage() {
    use serde_json::json;
    assert_eq!(ob::normalize_seen(None), Vec::<String>::new());
    assert_eq!(ob::normalize_seen(Some(&json!(null))), Vec::<String>::new());
    // Not a list — we start from a clean slate rather than crash.
    assert_eq!(ob::normalize_seen(Some(&json!("welcome"))), Vec::<String>::new());
    assert_eq!(ob::normalize_seen(Some(&json!(7))), Vec::<String>::new());
    assert_eq!(
        ob::normalize_seen(Some(&json!(["welcome", "finish"]))),
        vec!["welcome".to_string(), "finish".to_string()]
    );
    // Non-string elements are converted to a string, as `str(x)` did.
    assert_eq!(ob::normalize_seen(Some(&json!([1, true]))), vec!["1", "true"]);
}

// ── card style: colors from the skin, numbers from the core ─────────────

/// The overlay palette follows the SELECTED skin. Previously all three colors were
/// baked in for `steam_dark`, and a person who chose «Light» got
/// a dark blue card in the middle of a light window.
#[test]
fn overlay_palette_follows_the_skin() {
    let dark = ob::overlay_palette_for("steam_dark");
    assert_eq!(dark.halo, ob::HALO_COLOR, "неон скина по умолчанию");
    assert_eq!(dark.card_background, ob::CARD_BACKGROUND, "фон карточки");

    for id in ["light", "synthwave"] {
        let p = ob::overlay_palette_for(id);
        assert_ne!(p.halo, dark.halo, "{id}: неон обязан отличаться");
        assert_ne!(
            p.card_background, dark.card_background,
            "{id}: фон карточки обязан отличаться"
        );
    }
    // An unknown skin does not leave the card without colors.
    let unknown = ob::overlay_palette_for("нет-такого");
    assert_eq!(unknown, dark, "откат к скину по умолчанию");
}

/// No combination makes the text invisible: the text on the card does not
/// match its background, and the text on the accent button does not match the accent.
/// This is exactly what broke with the baked palette, so the check goes over
/// ALL skins at once, not over one.
#[test]
fn no_skin_makes_the_card_unreadable() {
    for (id, _) in rexon_core::ui_base::available_skins() {
        let p = ob::overlay_palette_for(id);
        assert_ne!(p.card_text, p.card_background, "{id}: текст слился с фоном");
        assert_ne!(p.card_muted, p.card_background, "{id}: подпись слилась с фоном");
        assert_ne!(p.on_accent, p.halo_hex, "{id}: надпись слилась с кнопкой");
        // Hover must be NOTICEABLE: on the default skin
        // the muted and the main text are the same (only white on
        // dark), and the former switch to white changed nothing.
        assert_ne!(p.halo_hex, p.card_muted, "{id}: наведение на «Пропустить» невидимо");
        assert_ne!(p.halo_hover, p.halo_hex, "{id}: наведение на «Далее» невидимо");
        // The text on the accent button is chosen by the brightness of the accent itself.
        let (r, g, b) = p.halo;
        let want = if rexon_core::ui_base::is_dark_rgb(r, g, b) {
            ob::ON_ACCENT_LIGHT
        } else {
            ob::ON_ACCENT_DARK
        };
        assert_eq!(p.on_accent, want, "{id}: контраст надписи на кнопке");
    }
}

/// The card's font sizes are computed by the core, and the lower bounds hold.
#[test]
fn card_point_sizes_keep_their_floor() {
    assert_eq!(ob::card_point_sizes(10), (9, 14));
    assert_eq!(ob::card_point_sizes(20), (19, 23));
    // A tiny system font size does not push the label and title below the threshold.
    assert_eq!(
        ob::card_point_sizes(1),
        (ob::PROGRESS_POINT_MIN, ob::TITLE_POINT_MIN)
    );
}

/// Style sheets are built from the same numbers and colors the palette returns —
/// no second copy is left in the Qt shim.
#[test]
fn card_qss_is_built_from_the_palette() {
    let p = ob::overlay_palette_for("light");
    let card = ob::card_qss(&p);
    assert!(card.contains(&p.card_background), "фон карточки не из палитры");
    assert!(card.contains(&p.halo_hex), "рамка карточки не из палитры");
    assert!(
        card.contains(&format!("border-radius: {}px", ob::CARD_RADIUS)),
        "радиус карточки не из ядра"
    );
    let next = ob::next_button_qss(&p);
    assert!(next.contains(p.on_accent), "надпись кнопки не из палитры");
    assert!(next.contains(&p.halo_hover), "наведение кнопки не из палитры");
    assert!(
        next.contains(&format!("padding: {}px {}px", ob::NEXT_PADDING.0, ob::NEXT_PADDING.1)),
        "отступы кнопки не из ядра"
    );
    let skip = ob::skip_button_qss(&p);
    assert!(skip.contains(&p.card_muted), "цвет «Пропустить» не из палитры");
    assert!(skip.contains(&p.halo_hex), "наведение «Пропустить» не из палитры");
}

// ── Session over the settings file (shared by the Rust and Python windows) ─

/// A settings manager on a TEMPORARY file: the checks do not touch the person's profile.
fn scratch_settings(name: &str) -> rexon_core::settings::SettingsManager {
    // Check name + PID: the neighbouring checks in this tree do the same
    // (`aeroshell_live_stand`, `app_parity`, `completeness_live_stand`), and for a
    // reason — without the PID two simultaneous `cargo test-core` runs take one and
    // the same directory, and the `remove_dir_all` of one wipes it from under the other.
    // A caught case: «No such file or directory» when writing settings,
    // when the run went in parallel with someone else's.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("onboarding_session")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_file = dir.join("settings.json");
    rexon_core::settings::SettingsManager {
        settings: rexon_core::settings::load_settings(&config_file),
        config_dir: dir,
        config_file,
    }
}

/// Rereading the same file anew is the "second run".
fn reopen(m: &rexon_core::settings::SettingsManager) -> rexon_core::settings::SettingsManager {
    rexon_core::settings::SettingsManager {
        settings: rexon_core::settings::load_settings(&m.config_file),
        config_dir: m.config_dir.clone(),
        config_file: m.config_file.clone(),
    }
}

#[test]
fn session_first_run_asks_and_refusal_sticks() {
    let mut m = scratch_settings("refusal");
    assert_eq!(ob::session_start(&m), ob::Start::Ask);
    let (start, saved) = ob::session_answer(&mut m, false);
    saved.unwrap();
    assert_eq!(start, ob::Start::Nothing);
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Nothing);
}

#[test]
fn session_walkthrough_is_not_shown_twice() {
    let mut m = scratch_settings("walkthrough");
    let (start, saved) = ob::session_answer(&mut m, true);
    saved.unwrap();
    assert_eq!(start, ob::Start::Show("welcome"));

    let mut current = "welcome";
    let mut shown = vec![current];
    loop {
        let (next, saved) = ob::session_advance(&mut m, current);
        saved.unwrap();
        match next {
            Some(step) => {
                current = step.id;
                shown.push(current);
            }
            None => break,
        }
    }
    let all: Vec<&str> = ob::STEPS.iter().map(|s| s.id).collect();
    assert_eq!(shown, all);
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Nothing);
}

/// A window closed halfway continues from the same place.
#[test]
fn session_progress_is_saved_after_each_step() {
    let mut m = scratch_settings("progress");
    ob::session_answer(&mut m, true).1.unwrap();
    ob::session_advance(&mut m, "welcome").1.unwrap();
    assert_eq!(
        ob::session_start(&reopen(&m)),
        ob::Start::Show("tab_appearance")
    );
}

#[test]
fn session_skip_and_replay() {
    let mut m = scratch_settings("skip");
    ob::session_answer(&mut m, true).1.unwrap();
    ob::session_skip(&mut m).unwrap();
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Nothing);

    // A repeat from Settings turns the tutorial on even after a refusal.
    ob::session_answer(&mut m, false).1.unwrap();
    let (start, saved) = ob::session_replay(&mut m);
    saved.unwrap();
    assert_eq!(start, ob::Start::Show("welcome"));
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Show("welcome"));
}

/// Keys and value shapes are those the Python window writes (`Tour._save_seen`):
/// the file is shared, and if they diverged a person would see the tutorial twice.
#[test]
fn session_reads_what_the_python_window_wrote() {
    let m = scratch_settings("python");
    std::fs::write(
        &m.config_file,
        r#"{"onboarding_enabled": true, "onboarding_seen": ["welcome", "tab_appearance"]}"#,
    )
    .unwrap();
    assert_eq!(
        ob::session_start(&reopen(&m)),
        ob::Start::Show("appearance_catalog")
    );
    std::fs::write(&m.config_file, r#"{"onboarding_enabled": "да"}"#).unwrap();
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Ask);
}

// ── Translation: Russian after moving the strings into the catalog ──────────
//
// The tutorial strings used to be hard-coded in Russian; now the source language is English, and
// Russian lives in `po/ru.po`. The check keeps the promise: for the Russian interface
// nothing changed. Below are the VERBATIM texts before the move.

const RU_BEFORE: &[(&str, &str, &str)] = &[
    (
        "welcome",
        "Добро пожаловать в Rexon",
        "Rexon меняет внешний вид рабочего стола: темы, обои, курсоры, шрифты. Покажу за \
         минуту, что где лежит. Можно пропустить и включить позже в Настройках.",
    ),
    (
        "tab_appearance",
        "Оформление",
        "Главная вкладка. Здесь каталог тем, обои, курсоры и шрифты. Тему можно скачать и \
         применить в один клик — искать по сайтам и распаковывать архивы вручную не нужно.",
    ),
    (
        "appearance_catalog",
        "Каталог тем",
        "Слева выбор раздела, справа — карточки тем с превью. Кнопка на карточке скачивает \
         тему и сразу применяет её. Прежний вид Rexon сохраняет, так что вернуться всегда \
         можно.",
    ),
    (
        "tab_wallpaper_engine",
        "Живые обои",
        "Видео и анимированные обои — то же, что Wallpaper Engine на Windows. Здесь же \
         мастерская: обои можно скачивать и ставить прямо из неё.",
    ),
    (
        "tab_desktops",
        "Рабочие столы",
        "Установка и переключение рабочих столов — KDE, GNOME, Cinnamon и других. Текущий \
         рабочий стол удалить нельзя: иначе после перезагрузки было бы некуда войти.",
    ),
    (
        "settings_protection",
        "Защита приложений",
        "Смена рабочего стола любит сбрасывать приложения по умолчанию — браузер, терминал, \
         просмотрщик. Защита в Настройках запоминает ваш выбор и возвращает его. Она \
         включена сразу.",
    ),
    (
        "tab_steam",
        "Возможности Steam",
        "Видеозаставка при входе в систему — как на Steam Deck — и фоны профиля Steam. \
         Ролик можно выбрать из тех, что уже есть в вашей библиотеке.",
    ),
    (
        "finish",
        "Готово",
        "Это всё. Обучение можно пройти заново или выключить в Настройках. Если что-то \
         сломается с оформлением — в меню «Пуск» есть пункт аварийного восстановления.",
    ),
];

/// Compile `po/<lang>.po` with the real `msgfmt` and read it with the same reader
/// as the product.
fn compiled_catalog(lang: &str) -> i18n::Catalog {
    let po = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../po/{lang}.po"));
    let out = std::env::temp_dir().join(format!("rexon-onboarding-{lang}-{}.mo", std::process::id()));
    let status = std::process::Command::new("msgfmt")
        .arg("--check")
        .arg("-o")
        .arg(&out)
        .arg(&po)
        .status()
        .expect("msgfmt (пакет gettext) запускается");
    assert!(status.success(), "msgfmt отверг {}", po.display());
    let catalog = i18n::Catalog::load(&out).expect("каталог читается");
    let _ = std::fs::remove_file(&out);
    catalog
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let ru = compiled_catalog("ru");
    let t = |msgid: &str| ru.get(None, msgid).unwrap_or_else(|| panic!("нет перевода «{msgid}»")).to_string();
    for (id, title, text) in RU_BEFORE {
        let step = ob::step(id).unwrap();
        assert_eq!(t(step.title), *title, "{id}: заголовок");
        assert_eq!(t(step.text), *text, "{id}: текст");
    }
    for (msgid, before) in [
        ("Show a short tour?", "Показать краткое обучение?"),
        (
            "It takes about a minute: I'll show where to change themes, wallpapers and \
             appearance. You can turn this on or off later in Settings.",
            "Займёт около минуты: покажу, где менять темы, обои и оформление. Позже это можно \
             включить или выключить в Настройках.",
        ),
        ("Show", "Показать"),
        ("No, thanks", "Не нужно"),
        ("Next", "Далее"),
        ("Done", "Готово"),
        ("Skip", "Пропустить"),
        ("Tour on first launch", "Обучение при первом запуске"),
        ("Take the tour again", "Пройти обучение заново"),
    ] {
        assert_eq!(t(msgid), before, "«{msgid}»");
    }
    assert_eq!(i18n::fill(&t("Step %1 of %2"), &[&3, &8]), "Шаг 3 из 8");
}
