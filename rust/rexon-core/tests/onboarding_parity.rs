//! Тесты сценария обучения при первом запуске.
//!
//! Оригинала в Python нет — модуль написан сразу на Rust, поэтому здесь не
//! golden-сверка, а проверка самих правил: порядок шагов, «показывать один
//! раз», отличие первого запуска от осознанного отказа, устойчивость
//! идентификаторов.

use rexon_core::i18n;
use rexon_core::onboarding as ob;

/// Тексты сверяются на ИСХОДНОМ языке: иначе ответ зависел бы от локали
/// машины и от того, стоит ли в системе пакет с каталогами. Русский
/// сверяется отдельно, по самому каталогу — `russian_catalog_keeps_old_texts`.
fn english() {
    i18n::set_language(Some(i18n::SOURCE_LANGUAGE));
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

/// Идентификаторы шагов — часть сохранённого состояния у пользователей.
/// Переименование заставит уже пройденный урок показаться заново, поэтому
/// список закреплён тестом намеренно.
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

/// Каждый шаг привязан к существующей вкладке главного окна либо к окну
/// целиком. Опечатка в идентификаторе вкладки означала бы подсказку,
/// указывающую в пустоту.
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
    // Первый и последний шаги — карточки по центру, без привязки.
    assert_eq!(ob::STEPS[0].anchor, ob::Anchor::Window);
    assert_eq!(ob::STEPS[ob::STEPS.len() - 1].anchor, ob::Anchor::Window);
}

#[test]
fn texts_are_filled() {
    for step in ob::STEPS {
        assert!(!step.title.trim().is_empty(), "шаг {}: пустой заголовок", step.id);
        assert!(step.text.len() > 40, "шаг {}: текст слишком короткий", step.id);
        // Жаргон в обучении недопустим — оно как раз для тех, кто его не знает.
        for word in ["QSS", "kpackage", "layer-shell", "mmap", "D-Bus"] {
            assert!(
                !step.text.contains(word),
                "шаг {}: жаргон «{word}» в тексте для пользователя",
                step.id
            );
        }
    }
}

/// Первый запуск отличается от осознанного отказа: иначе человек,
/// однажды сказавший «не нужно», получал бы вопрос каждый раз.
#[test]
fn start_distinguishes_first_run_from_refusal() {
    assert_eq!(ob::on_start(None, &[]), ob::Start::Ask);
    assert_eq!(ob::on_start(Some(false), &[]), ob::Start::Nothing);
    assert_eq!(ob::on_start(Some(true), &[]), ob::Start::Show("welcome"));
    // Обучение включено, но частично пройдено — продолжаем с места.
    assert_eq!(
        ob::on_start(Some(true), &ids(&["welcome", "tab_appearance"])),
        ob::Start::Show("appearance_catalog")
    );
    // Пройдено целиком — не мешаем.
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

    // Повторная пометка не плодит записей.
    let before = seen.len();
    ob::mark_seen(&mut seen, "welcome");
    assert_eq!(seen.len(), before);
}

/// «Далее» идёт вперёд по сценарию, а не возвращается к пропущенному
/// раньше шагу — иначе обучение ходило бы кругами.
#[test]
fn next_button_moves_forward() {
    let seen = ids(&["tab_appearance"]);
    assert_eq!(
        ob::step_after("welcome", &seen).map(|s| s.id),
        Some("appearance_catalog"),
        "уже показанный шаг должен пропускаться"
    );
    // С последнего шага идти некуда.
    assert!(ob::step_after("finish", &[]).is_none());
    // Неизвестный идентификатор (состояние от старой версии) не роняет
    // обучение — начинаем с первого непоказанного.
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

// ── Накладка: геометрия, карточка, наведение ───────────────────────────────
//
// Эталон — код `_Spotlight`/`Tour._show`/`Tour._aim` на день переноса.
// ⛔ Qt не стартует: всё здесь — чистая арифметика и таблица шагов.

#[test]
fn spotlight_hole_pads_the_target() {
    // Вырез шире цели на отступ с каждой стороны.
    assert_eq!(
        ob::spotlight_hole(Some((100, 50, 200, 30))),
        Some((100 - 8, 50 - 8, 200 + 16, 30 + 16))
    );
    assert_eq!(ob::TARGET_PAD, 8);
    // Цели нет либо она вырожденная — затемняем окно целиком.
    assert_eq!(ob::spotlight_hole(None), None);
    assert_eq!(ob::spotlight_hole(Some((10, 10, 0, 20))), None);
    assert_eq!(ob::spotlight_hole(Some((10, 10, 20, -1))), None);
}

#[test]
fn card_goes_below_the_hole_when_it_fits() {
    // Окно 1000×700, вырез сверху — карточка под ним с зазором.
    let p = ob::card_placement((1000, 700), (400, 200), Some((100, 40, 200, 60)));
    assert_eq!(p.y, 40 + 60 - 1 + ob::CARD_GAP);
    // По горизонтали — по центру выреза, но не ближе зазора к краю окна:
    // здесь центр дал бы ровно 0, и карточка прижалась бы к самой рамке.
    assert_eq!(p.x, ob::CARD_GAP);
    // Вырез посередине окна — центрирование видно без клампа.
    let p = ob::card_placement((1000, 700), (400, 200), Some((400, 40, 200, 60)));
    assert_eq!(p.x, (400 + 200 / 2) - 400 / 2);
}

#[test]
fn card_goes_above_when_there_is_no_room_below() {
    // Вырез у самого низа: под ним карточка не помещается.
    let p = ob::card_placement((1000, 700), (400, 200), Some((100, 600, 200, 60)));
    assert_eq!(p.y, 600 - ob::CARD_GAP - 200);
    assert!(p.y >= 0);
}

#[test]
fn card_centers_when_it_fits_nowhere() {
    // Вырез во весь экран — ни сверху, ни снизу места нет.
    let p = ob::card_placement((1000, 700), (400, 600), Some((0, 0, 1000, 700)));
    assert_eq!(p.y, (700 - 600) / 2);
    // Без выреза карточка ровно по центру.
    let p = ob::card_placement((1000, 700), (400, 200), None);
    assert_eq!((p.x, p.y), ((1000 - 400) / 2, (700 - 200) / 2));
}

#[test]
fn card_never_hangs_off_the_window_edge() {
    // Вырез у левого края — карточка прижимается с зазором, а не уезжает
    // за границу окна.
    let p = ob::card_placement((1000, 700), (400, 200), Some((0, 40, 40, 40)));
    assert_eq!(p.x, ob::CARD_GAP);
    // Тот же случай справа.
    let p = ob::card_placement((1000, 700), (400, 200), Some((960, 40, 40, 40)));
    assert_eq!(p.x, 1000 - 400 - ob::CARD_GAP);
    // Карточка шире окна: прижать некуда — но за левый край не уводим.
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

    // На последнем шаге отказываться уже не от чего (правило #13).
    let last = ob::card_content("finish").unwrap();
    assert_eq!(last.progress, format!("Step {} of {}", ob::total_steps(), ob::total_steps()));
    assert_eq!(last.next_text, ob::btn_done());
    assert_eq!(last.skip_text, "");
    assert!(!last.skip_visible);

    // Незнакомый шаг — пустой карточки быть не должно.
    assert!(ob::card_content("нет-такого").is_none());
}

#[test]
fn every_step_has_content_and_an_aim() {
    for step in ob::STEPS {
        let c = ob::card_content(step.id).unwrap_or_else(|| panic!("нет карточки {}", step.id));
        assert!(!c.title.is_empty(), "{}: пустой заголовок", step.id);
        assert!(!c.text.is_empty(), "{}: пустой текст", step.id);
        assert!(!c.progress.is_empty(), "{}: пустая подпись", step.id);
        // Наведение согласовано с привязкой шага.
        let aim = ob::aim_target(step.id);
        match step.anchor {
            ob::Anchor::Tab(id) => assert_eq!(aim, ob::AimTarget::TabHeader(id)),
            ob::Anchor::TabContent(id) => assert_eq!(aim, ob::AimTarget::TabContent(id)),
            ob::Anchor::Window => assert_eq!(aim, ob::AimTarget::WholeWindow),
        }
    }
    // Незнакомый шаг не рисует пустую рамку.
    assert_eq!(ob::aim_target("нет-такого"), ob::AimTarget::WholeWindow);
}

#[test]
fn seen_from_settings_survives_garbage() {
    use serde_json::json;
    assert_eq!(ob::normalize_seen(None), Vec::<String>::new());
    assert_eq!(ob::normalize_seen(Some(&json!(null))), Vec::<String>::new());
    // Не список — начинаем с чистого листа, а не падаем.
    assert_eq!(ob::normalize_seen(Some(&json!("welcome"))), Vec::<String>::new());
    assert_eq!(ob::normalize_seen(Some(&json!(7))), Vec::<String>::new());
    assert_eq!(
        ob::normalize_seen(Some(&json!(["welcome", "finish"]))),
        vec!["welcome".to_string(), "finish".to_string()]
    );
    // Нестроковые элементы приводятся к строке, как это делал `str(x)`.
    assert_eq!(ob::normalize_seen(Some(&json!([1, true]))), vec!["1", "true"]);
}

// ── стиль карточки: цвета от скина, числа из ядра ───────────────────────

/// Палитра накладки следует ВЫБРАННОМУ скину. Раньше все три цвета были
/// запечены под `steam_dark`, и человек, выбравший «Светлый», получал
/// тёмно-синюю карточку посреди светлого окна.
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
    // Незнакомый скин не оставляет карточку без цветов.
    let unknown = ob::overlay_palette_for("нет-такого");
    assert_eq!(unknown, dark, "откат к скину по умолчанию");
}

/// Ни одно сочетание не делает надпись невидимой: текст на карточке не
/// совпадает с её фоном, а надпись на акцентной кнопке — с акцентом.
/// Именно это ломалось при запечённой палитре, поэтому проверка идёт по
/// ВСЕМ скинам сразу, а не по одному.
#[test]
fn no_skin_makes_the_card_unreadable() {
    for (id, _) in rexon_core::ui_base::available_skins() {
        let p = ob::overlay_palette_for(id);
        assert_ne!(p.card_text, p.card_background, "{id}: текст слился с фоном");
        assert_ne!(p.card_muted, p.card_background, "{id}: подпись слилась с фоном");
        assert_ne!(p.on_accent, p.halo_hex, "{id}: надпись слилась с кнопкой");
        // Наведение обязано быть ЗАМЕТНЫМ: на скине по умолчанию
        // приглушённый и основной текст совпадают (только белый на
        // тёмном), и прежний переход в белый ничего не менял.
        assert_ne!(p.halo_hex, p.card_muted, "{id}: наведение на «Пропустить» невидимо");
        assert_ne!(p.halo_hover, p.halo_hex, "{id}: наведение на «Далее» невидимо");
        // Надпись на акцентной кнопке выбрана по яркости самого акцента.
        let (r, g, b) = p.halo;
        let want = if rexon_core::ui_base::is_dark_rgb(r, g, b) {
            ob::ON_ACCENT_LIGHT
        } else {
            ob::ON_ACCENT_DARK
        };
        assert_eq!(p.on_accent, want, "{id}: контраст надписи на кнопке");
    }
}

/// Кегли карточки считает ядро, и нижние границы держатся.
#[test]
fn card_point_sizes_keep_their_floor() {
    assert_eq!(ob::card_point_sizes(10), (9, 14));
    assert_eq!(ob::card_point_sizes(20), (19, 23));
    // Крошечный системный кегль не роняет подпись и заголовок ниже порога.
    assert_eq!(
        ob::card_point_sizes(1),
        (ob::PROGRESS_POINT_MIN, ob::TITLE_POINT_MIN)
    );
}

/// Таблицы стилей собраны из тех же чисел и цветов, что отдаёт палитра, —
/// второй копии в Qt-шиме не осталось.
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

// ── Сеанс поверх файла настроек (общего для Rust- и Python-окна) ─────────

/// Менеджер настроек на ВРЕМЕННОМ файле: профиль человека сверки не трогают.
fn scratch_settings(name: &str) -> rexon_core::settings::SettingsManager {
    // Имя сверки + PID: соседние сверки в этом дереве делают так же
    // (`aeroshell_live_stand`, `app_parity`, `completeness_live_stand`), и по
    // делу — без PID два одновременных прогона `cargo test-core` берут один и
    // тот же каталог, и `remove_dir_all` одного стирает его из-под другого.
    // Пойманный случай: «No such file or directory» при записи настроек,
    // когда прогон шёл параллельно с чужим.
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

/// Перечитать тот же файл заново — это «второй запуск».
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

/// Закрытое на середине окно продолжает с того же места.
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

    // Повтор из Настроек включает обучение даже после отказа.
    ob::session_answer(&mut m, false).1.unwrap();
    let (start, saved) = ob::session_replay(&mut m);
    saved.unwrap();
    assert_eq!(start, ob::Start::Show("welcome"));
    assert_eq!(ob::session_start(&reopen(&m)), ob::Start::Show("welcome"));
}

/// Ключи и вид значений — те, что пишет Python-окно (`Tour._save_seen`):
/// файл общий, и разойдись они — человек увидел бы обучение дважды.
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

// ── Перевод: русский после переноса строк в каталог ─────────────────────────
//
// Строки обучения были зашиты по-русски; теперь исходный язык английский, а
// русский живёт в `po/ru.po`. Сверка держит слово: для русского интерфейса
// ничего не поменялось. Ниже — ДОСЛОВНЫЕ тексты до переноса.

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

/// Собрать `po/<lang>.po` настоящим `msgfmt` и прочитать тем же читателем,
/// что и продукт.
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
