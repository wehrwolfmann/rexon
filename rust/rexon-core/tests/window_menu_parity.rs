//! Сверка МЕНЮ главного окна: ядро `rexon_core::window_menu` (пункты,
//! «О программе», «Восстановить тему KDE», «Удалить Rexon») и его разметка
//! в `rexon-gui`.
//!
//! ЧТО ЗДЕСЬ ЭТАЛОН (с 02.10.2026). Python в Rexon — только архив: он не
//! эталон и не мост. Прежде каждая строка ядра обязана была стоять в живом
//! `rexon/qt_main/window.py`; теперь эталон — текущие тексты и строй самого
//! ядра, записанные здесь дословно, и снимок каркаса
//! `golden/qt_main_cases.json` (пункты меню). Правка текста ядра красит
//! сверку, пока эталон не поправлен осознанно.
//!
//! ЧЕГО СВЕРКА НЕ МЕРИТ. Вид меню: открылось ли оно, построились ли делегаты,
//! ходит ли по нему клавиатура. Это мерит прогон окна
//! (`rexon-main-window --selftest`, строки `REXON-MAINWINDOW-MENU*`) и глаз.

use std::fs;
use std::path::Path;

use rexon_core::tray;
use rexon_core::window_menu as wm;
use serde_json::Value;

mod i18n_pin;

fn source(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("нет исходника {path:?}: {e}"))
}

/// Убрать всё, что различается только записью: пробелы и переводы строк.
fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/qt_main_cases.json")).unwrap()
}

// ============================================================================
// Состав меню
// ============================================================================

/// Кнопка «☰» и четыре пункта в порядке продукта.
#[test]
fn menu_items_match_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::MENU_BUTTON, "☰");

    let items = wm::menu_items();
    assert_eq!(items.len(), 4, "в меню продукта ровно четыре пункта");
    let shortcuts: Vec<&str> = items.iter().map(|i| i.shortcut).collect();
    assert_eq!(shortcuts, ["", "", "", "Ctrl+Q"], "горячие клавиши пунктов меню");

    // Порядок — не вкусовщина: «Выход» стоит последним, за разделителем,
    // потому что это единственный необратимый пункт без вопроса.
    let ids: Vec<&str> = items.iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        vec![wm::ID_RECOVER_KDE, wm::ID_ABOUT, wm::ID_UNINSTALL, wm::ID_QUIT],
        "порядок пунктов меню разошёлся с продуктом"
    );

    // Со снимком каркаса тоже: расхождение здесь означает, что ядро и
    // эталон каркаса описывают разное меню.
    let g = golden();
    let snapshot = g["menu"]["actions"].as_array().unwrap();
    let snapshot_labels: Vec<&str> = snapshot
        .iter()
        .map(|a| a["label"].as_str().unwrap())
        .collect();
    let core_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        core_labels, snapshot_labels,
        "пункты меню в ядре и в снимке каркаса разошлись"
    );
    assert_eq!(
        g["menu"]["button"].as_str().unwrap(),
        wm::MENU_BUTTON,
        "кнопка меню в ядре и в снимке каркаса разошлась"
    );
}

/// Второй копии «Выхода» в проекте нет: подпись со значком собирает лоток.
#[test]
fn quit_label_has_no_second_copy() {
    for pin in [i18n_pin::ru as fn() -> std::sync::MutexGuard<'static, ()>, i18n_pin::en] {
        let _lang = pin();
        let quit = wm::menu_items()
            .into_iter()
            .find(|i| i.id == wm::ID_QUIT)
            .expect("пункт «Выход» пропал из меню");
        assert_eq!(
            quit.label,
            tray::titled("quit"),
            "«Выход» в меню окна и в лотке разошёлся"
        );
    }
}

/// Разделитель ровно один и стоит перед «Выходом» — как в `_build_menubar`.
#[test]
fn the_only_separator_stands_before_quit() {
    let _lang = i18n_pin::ru();
    let with_sep: Vec<&str> = wm::menu_items()
        .iter()
        .filter(|i| i.separator_before)
        .map(|i| i.id)
        .collect();
    assert_eq!(with_sep, vec![wm::ID_QUIT]);
}

/// Кнопка «☰» стоит СПРАВА, а подменю под ней раскладывается слева направо.
///
/// Место кнопки не косметика: слева от неё вплотную начинается полоса
/// разделов, и кнопка, поставленная там же, читается как первый раздел.
/// Решение живёт в `qml/WindowMenu.qml`: `contentItem` полосы меню
/// раскладывается `Qt.RightToLeft`, а подменю — отдельное окно — раскладки
/// полосы не наследует и остаётся слева направо (зеркалить его нечем:
/// `LayoutMirroring` в разметке не включается).
#[test]
fn menu_button_stands_on_the_right_and_the_popup_reads_left_to_right() {
    let qml = source("rust/rexon-gui/qml/WindowMenu.qml");
    let code: String = qml
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("layoutDirection: Qt.RightToLeft"),
        "кнопка «☰» больше не стоит справа"
    );
    assert!(
        !code.contains("LayoutMirroring"),
        "в полосе меню включили зеркалирование — развернётся и подменю"
    );
}

/// Недоступных пунктов в продукте нет — и выдумывать их нельзя.
#[test]
fn no_menu_item_is_disabled_in_the_product() {
    let _lang = i18n_pin::ru();
    for item in wm::menu_items() {
        assert!(wm::is_enabled(item.id), "пункт «{}» стал недоступен", item.id);
    }
}

// ============================================================================
// «О программе»
// ============================================================================

#[test]
fn about_text_matches_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::about_title(), "О программе Rexon");
    assert_eq!(
        wm::about_html(),
        format!(
            "<h2>Rexon {}</h2><p>Менеджер рабочих столов для Linux</p>\
             <p>Установка, удаление и настройка рабочих столов, тем, иконок, курсоров, \
             шрифтов, обоев (включая Wallpaper&nbsp;Engine), экрана входа и загрузочной \
             заставки (SDDM/Plymouth), виджетов и расширений — всё в одном окне.</p>\
             <p><b>Поддержка:</b> KDE, GNOME, Cinnamon, XFCE, MATE, Budgie, Pantheon, Deepin, LXQt, \
             COSMIC, Wayfire, Hyprland, sway, niri, River</p>\
             <p>© 2026 Wehrwolfmann · лицензия Apache 2.0</p>\
             <p style='color:#888;font-size:11px'>Использует Qt (LGPL-3.0) и \
             KDE&nbsp;Frameworks (LGPL-2.1+) — динамическая линковка.</p>",
            wm::version_string()
        )
    );
}

/// Абзац о возможностях переписан 01.10.2026 с жаргона («настройка DE…
/// (вкл. …) SDDM/Plymouth… — через единый интерфейс») обычными словами: те
/// же разделы в том же порядке, без сокращений.
#[test]
fn about_features_reworded_on_purpose() {
    let _lang = i18n_pin::ru();
    let html = wm::about_html();
    let want = "<p>Установка, удаление и настройка рабочих столов, тем, иконок, курсоров, \
                шрифтов, обоев (включая Wallpaper&nbsp;Engine), экрана входа и загрузочной \
                заставки (SDDM/Plymouth), виджетов и расширений — всё в одном окне.</p>";
    assert!(html.contains(want), "{html}");
    let mut at = 0;
    for item in ["рабочих столов", "тем", "иконок", "курсоров", "шрифтов", "обоев",
                 "Wallpaper&nbsp;Engine", "SDDM", "Plymouth", "виджетов", "расширений"] {
        let pos = want[at..].find(item).unwrap_or_else(|| panic!("пропал раздел «{item}»"));
        at += pos + item.len();
    }
    assert!(!html.contains(" DE,") && !html.contains("вкл."), "жаргон вернулся: {html}");
}

/// Атрибуция LGPL — юридический текст, а не украшение интерфейса: обе
/// библиотеки берутся динамической линковкой, и указать это — условие их
/// использования при распространении Rexon. Пропажа этого абзаца из справки —
/// нарушение лицензии, а не косметика.
#[test]
fn about_keeps_the_lgpl_attribution() {
    let _lang = i18n_pin::ru();
    let html = wm::about_html();
    for token in ["LGPL-3.0", "LGPL-2.1+", "динамическая", "KDE&nbsp;Frameworks"] {
        assert!(html.contains(token), "из справки пропала атрибуция: {token}");
    }
}

// ============================================================================
// «О программе»: благодарности против NOTICE и README
// ============================================================================

/// Разделы NOTICE: (заголовок, текст до следующего заголовка). Заголовок —
/// строка, под которой стоит черта из одних «-».
fn notice_sections() -> Vec<(String, String)> {
    let text = source("NOTICE");
    let lines: Vec<&str> = text.lines().collect();
    let heads: Vec<usize> = (1..lines.len())
        .filter(|&i| {
            let l = lines[i].trim();
            l.len() >= 3 && l.chars().all(|c| c == '-') && !lines[i - 1].trim().is_empty()
        })
        .map(|i| i - 1)
        .collect();
    heads
        .iter()
        .enumerate()
        .map(|(n, &h)| {
            let end = heads.get(n + 1).copied().unwrap_or(lines.len());
            (lines[h].to_string(), lines[h + 2..end].join("\n"))
        })
        .collect()
}

/// Подходит ли заголовок раздела NOTICE к записи таблицы: `notice_heading`
/// с «\n» на конце — заголовок целиком, иначе — начало заголовка.
fn heading_matches(heading: &str, key: &str) -> bool {
    match key.strip_suffix('\n') {
        Some(exact) => heading == exact,
        None => heading.starts_with(key),
    }
}

/// Разделы NOTICE, которых в благодарностях нет НАМЕРЕННО — с причиной.
/// Новый раздел NOTICE без записи здесь или в `THIRD_PARTY` красит сверку:
/// значит, в поставке появилась чужая программа, а справка о ней молчит.
const NOTICE_NOT_IN_ABOUT: &[(&str, &str)] = &[
    ("gemmology ", "часть fxtranslate, благодарность — fxtranslate"),
    ("xsimd ", "часть fxtranslate, благодарность — fxtranslate"),
    ("Noto Sans ", "подмножество шрифта для заставки, не программа"),
    ("Снимки рабочих столов", "картинки карусели, не программа"),
];

/// Таблица благодарностей и NOTICE не расходятся: у каждой записи таблицы
/// есть раздел NOTICE с той же ссылкой и той же лицензией, и у каждого
/// раздела NOTICE есть запись в таблице (или причина, почему её нет).
#[test]
fn about_credits_match_notice() {
    let sections = notice_sections();
    assert!(sections.len() >= 10, "NOTICE не разобрался на разделы: {}", sections.len());

    for c in wm::THIRD_PARTY {
        let found: Vec<&(String, String)> = sections
            .iter()
            .filter(|(h, _)| heading_matches(h, c.notice_heading))
            .collect();
        assert_eq!(found.len(), 1, "«{}»: в NOTICE {} разделов «{}»", c.name, found.len(),
                   c.notice_heading.trim_end());
        let (heading, body) = found[0];
        assert!(body.contains(c.url), "«{}»: в разделе NOTICE «{heading}» нет ссылки {}", c.name, c.url);
        for part in c.license.split(" / ") {
            assert!(body.contains(part),
                    "«{}»: в разделе NOTICE «{heading}» нет лицензии {part}", c.name);
        }
    }

    for (heading, _) in &sections {
        let in_table = wm::THIRD_PARTY.iter().any(|c| heading_matches(heading, c.notice_heading));
        let excused = NOTICE_NOT_IN_ABOUT.iter().any(|(k, _)| heading.starts_with(k));
        assert!(in_table || excused,
                "раздел NOTICE «{heading}» не попал в благодарности «О программе» \
                 (window_menu::THIRD_PARTY) и не объяснён в NOTICE_NOT_IN_ABOUT");
    }
}

/// Тот же список стоит в разделе «Credits» README — имена не разошлись.
#[test]
fn about_credits_match_readme() {
    let readme = source("README.md");
    let start = readme.find("\n## Credits").expect("в README нет раздела Credits");
    let rest = &readme[start + 1..];
    let credits = &rest[..rest[3..].find("\n## ").map_or(rest.len(), |p| p + 3)];
    for c in wm::THIRD_PARTY {
        assert!(credits.contains(c.name), "«{}» нет в разделе Credits README", c.name);
    }
}

/// Благодарности в справке: каждая программа — ссылкой и с лицензией, а
/// тело окна — справка целиком и благодарности под ней.
#[test]
fn about_credits_show_links_and_licenses() {
    let _lang = i18n_pin::en();
    let html = wm::about_credits_html();
    for c in wm::THIRD_PARTY {
        assert!(html.contains(&format!("<a href='{}'>{}</a>", c.url, c.name)), "{}: {html}", c.name);
        assert!(html.contains(&format!("License: {}", c.license)), "{}: {html}", c.name);
        assert!(html.contains(c.purpose), "{}: нет строки «что делает»", c.name);
    }
    assert!(html.contains("NOTICE"), "{html}");
    assert_eq!(wm::about_dialog_html(), format!("{}{}", wm::about_html(), html));
}

/// Отказ от связи с Wallpaper Engine и Valve стоит в «О программе» и
/// дословно совпадает с README: марка «Wallpaper Engine» есть в интерфейсе,
/// и без оговорки её можно принять за знак партнёрства. Строка одна
/// (`window_menu::not_affiliated_text`), копии нет.
#[test]
fn about_and_readme_carry_the_same_disclaimer() {
    let _lang = i18n_pin::en();
    let line = wm::not_affiliated_text();
    assert_eq!(
        line,
        "Rexon is not affiliated with, endorsed by or sponsored by Wallpaper Engine, \
         its developers, or Valve."
    );
    assert!(wm::about_dialog_html().contains(&line), "в «О программе» нет отказа от связи");
    let readme = source("README.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(readme.contains(&line), "в README нет той же строки отказа от связи: «{line}»");
}

/// Версия в справке — та же, что у крейта (`version` рабочего пространства
/// в `rust/Cargo.toml`, её же берёт пакет): две копии номера разошлись бы
/// молча, а по номеру в справке человек сообщает о дефекте.
#[test]
fn about_version_matches_the_crate() {
    assert_eq!(
        wm::version_string(),
        env!("CARGO_PKG_VERSION"),
        "версия в справке (`constants::REXON_VERSION`) разошлась с версией крейта"
    );
}

// ============================================================================
// «Восстановить тему KDE»
// ============================================================================

#[test]
fn recover_kde_texts_match_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::recover_title(), "Восстановить тему KDE");
    assert_eq!(wm::RECOVER_FALLBACK_TARGET, "Breeze Dark (fallback)");
    assert_eq!(wm::RECOVER_BACKUP_REL, ".config/rexon/last_kde_lookandfeel.txt");
    assert_eq!(wm::recover_started_title(), "Восстановление запущено");
    assert_eq!(wm::error_title(), "Ошибка");
    assert_eq!(
        wm::recover_question("{target}"),
        "Будет применена тема: <b>{target}</b><br><br>Также <b>обои сбросятся на \
         стандартные KDE</b> (любые видео/живые обои остановятся) — рабочий стол \
         вернётся к виду «как после установки системы».<br><br>Используйте, если \
         применённая тема сломала систему (чёрные окна, белые элементы, программы \
         аварийно закрываются)."
    );
    assert_eq!(
        wm::recover_started_text("{target}"),
        "Применяется: {target}\n\nKDE Plasma переключит тему и сбросит обои на \
         стандартные, plasmashell перезапустится через 1-2 секунды."
    );
    assert_eq!(wm::recover_failed_text(""), "Не удалось запустить rexon: ");
}

/// Про обои предупреждает САМ вопрос. Сброс темы останавливает живые и
/// видео-обои; человек, не предупреждённый об этом, примет пропавшие обои за
/// поломку Rexon (правило #13).
#[test]
fn recover_question_warns_about_wallpapers() {
    let _lang = i18n_pin::ru();
    let q = wm::recover_question("Breeze");
    assert!(q.contains("обои сбросятся"), "вопрос молчит про обои: {q}");
    assert!(q.contains("Breeze"), "вопрос не назвал тему: {q}");
}

/// Команда сброса — тот же CLI-флаг, которым пользуется продукт, но зовётся
/// им СВОЙ бинарь окна (маршрут целиком на Rust), а не Python-вход.
#[test]
fn recover_uses_the_same_cli_flag() {
    assert_eq!(
        wm::recover_argv(Some("/x/rexon-main-window")),
        vec!["/x/rexon-main-window", "--recover-kde"]
    );
}

/// Перед сбросом темы окно проверяет пакеты-замены частей Plasma и идёт по
/// шагам ядра: окно с тремя кнопками, следующий шаг по кнопке, последний
/// вопрос с командой, итог. Своих слов и своих решений у разметки нет.
#[test]
fn plasma_forks_steps_come_from_the_core() {
    let _lang = i18n_pin::ru();
    // Окно спрашивает следующий шаг у ядра.
    let qml = source("rust/rexon-gui/qml/WindowMenuDialogs.qml");
    assert!(qml.contains("menuModel.forksNext("), "Rust-окно решает шаг само");
    for id in [wm::FORKS_BUTTON_REMOVE, wm::FORKS_BUTTON_THEME, wm::FORKS_BUTTON_CANCEL] {
        assert!(qml.contains(&format!("forksChoice(\"{id}\")")), "кнопка «{id}» без шага");
    }
    // Человеку — без жаргона: ни «форков», ни «стока», ни имени менеджера
    // пакетов в словах окна (команда в вопросе — отдельное поле).
    let prompt = wm::forks_prompt(&["x".into()]).unwrap();
    let mut words = vec![prompt.title, wm::forks_text(&["x".into()]), wm::forks_done_text()];
    words.extend(prompt.buttons.into_iter().map(|b| b.label));
    for text in &words {
        let lower = text.to_lowercase();
        for jargon in ["форк", "сток", "pacman"] {
            assert!(!lower.contains(jargon), "жаргон «{jargon}» в окне замен Plasma: {text}");
        }
    }
    // Смысл последнего вопроса тот же: один пароль, нужен повторный вход,
    // панель Win7 пропадёт.
    let confirm = wm::forks_confirm_text("cmd");
    for fact in ["пароль", "войти", "SevenStart"] {
        assert!(confirm.contains(fact), "вопрос перед удалением молчит про «{fact}»");
    }
    assert!(wm::forks_done_text().contains("Восстановить тему KDE"));
}

#[test]
fn recover_backup_path_is_built_from_home() {
    assert_eq!(
        wm::recover_backup_path("/home/кто-то", None),
        "/home/кто-то/.config/rexon/last_kde_lookandfeel.txt"
    );
    // Нестандартный XDG уводит копию за собой — вшитый `~/.config` читал бы
    // не тот файл и называл бы человеку не ту тему.
    assert_eq!(
        wm::recover_backup_path("/home/кто-то", Some("/подстава/конфиг")),
        "/подстава/конфиг/rexon/last_kde_lookandfeel.txt"
    );
}

// ============================================================================
// «Удалить Rexon»
// ============================================================================

#[test]
fn uninstall_dialog_texts_match_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::uninstall_title(), "Удалить Rexon");
    assert_eq!(wm::uninstall_text(), "Будут удалены файлы и директории Rexon");
    // Решение Wehrwolfmann 15.09.2026: «сохранить» оставляет настройки
    // вместе с резервными копиями, и слова ядра это говорят. Старые подписи
    // («сохранить бэкапы» — настройки при этом сносились) вернуться не должны.
    for old in ["Удалить (сохранить бэкапы)", "Сохранить резервные копии DE?"] {
        assert!(
            !wm::uninstall_keep_label().contains(old) && !wm::uninstall_informative(&[], &[]).contains(old),
            "ядро снова говорит «{old}», а «сохранить» теперь оставляет и настройки"
        );
    }
    assert_eq!(wm::uninstall_delete_label(), "Удалить всё");
    assert_eq!(wm::uninstall_cancel_label(), "Отмена");
    assert_eq!(wm::uninstall_done_title(), "Удаление завершено");
    assert_eq!(wm::uninstall_failed_title(), "Ошибка удаления");
    let informative = wm::uninstall_informative(&["/tmp/rexon".to_string()], &[]);
    assert!(informative.contains(&wm::uninstall_keep_hint()), "{informative}");
    // Блок путей — переносимый <pre> (`ui_base::dialog_pre`): в голом
    // длинный путь уходил за правый край окна (снимок 30.09.2026).
    assert!(
        informative.contains(&rexon_core::ui_base::dialog_pre("  • /tmp/rexon")),
        "перечень удаляемого не в переносимом блоке: {informative}"
    );
    assert!(
        informative.contains("  • /tmp/rexon"),
        "перечень удаляемого не попал в пояснение: {informative}"
    );
    assert_eq!(
        squash(&informative),
        squash(
            "<pre style=\"white-space: pre-wrap\">  • /tmp/rexon</pre>\
             <p>При любом выборе удаляются сама программа (список выше) и временные файлы Rexon.</p>\
             <p>«Удалить всё» удалит также настройки Rexon, резервные копии рабочих столов и \
             установленные обои, темы и дополнения.</p>\
             <p>Если обои или тема экрана входа, которые стоят сейчас, поставлены из Rexon, \
             «Удалить всё» снимет и их — рабочий стол вернётся к обычному виду.</p>\
             <p>«Удалить, но сохранить настройки и резервные копии» оставит на месте настройки, \
             резервные копии и установленные обои, темы и дополнения: если потом установите \
             Rexon снова, он продолжит с того же места.</p>"
        ),
        "пояснение окна удаления"
    );
    assert_eq!(wm::uninstall_done_text(""), "\n\nПриложение будет закрыто.");
}

/// Перечень режется на том же числе, что в продукте, и остаток считается.
#[test]
fn uninstall_file_list_is_cut_like_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::UNINSTALL_FILES_SHOWN, 15);

    let files: Vec<String> = (0..18).map(|i| format!("/tmp/rexon-{i}")).collect();
    let text = wm::uninstall_files_text(&files);
    assert_eq!(
        text.lines().count(),
        16,
        "показано не 15 путей плюс строка остатка: {text}"
    );
    assert!(text.ends_with("  • … ещё 3"), "остаток не посчитан: {text}");
}

/// Кнопка по умолчанию — «Отмена». Защита от дурака: случайный Enter не
/// должен снести программу с машины покупателя. Решает ядро, разметка
/// получает решение полем `defaultId`.
#[test]
fn cancel_is_the_default_button() {
    assert_eq!(wm::UNINSTALL_DEFAULT_ID, "cancel");
    let bridge = source("rust/rexon-gui/src/window_menu.rs");
    assert!(
        bridge.contains("(\"defaultId\", qstr(wm::UNINSTALL_DEFAULT_ID))"),
        "окно удаления берёт кнопку по умолчанию мимо ядра"
    );
}

/// Удаление ходит через ядро: детект владельца файла, `pkexec pacman -R`
/// для пакета дистрибутива и перечень путей. Второго движка удаления в
/// проекте нет.
#[test]
fn uninstall_reuses_the_core_installer_engine() {
    use rexon_core::installer;
    use rexon_core::system::PkgFamily;

    assert_eq!(
        installer::distro_pkg_owner(PkgFamily::Arch).unwrap(),
        vec!["pacman", "-Qoq"],
        "детект владельца файла на Arch сменился"
    );
    let remove = installer::distro_pkg_remove(PkgFamily::Arch).unwrap();
    let cmd = installer::pkg_remove_cmd(&remove, "rexon");
    assert_eq!(
        cmd.argv,
        vec!["pkexec", "pacman", "-R", "--noconfirm", "rexon"],
        "удаление пакета дистрибутива идёт мимо `pkexec pacman -R`"
    );
}

// ============================================================================
// Перевод: русский «как было» и английский
// ============================================================================
//
// Тексты меню и его окон были зашиты по-русски; теперь исходный язык
// английский, русский живёт в `po/ru.po`. Ниже — русский каталог целиком
// (окно удаления 15-16.09, перечень остатка), английский — отдельной
// проверкой.

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    let labels: Vec<String> = wm::menu_items().into_iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        ["🛟 Восстановить тему KDE", "ℹ️ О программе", "🗑️ Удалить Rexon", "🚪 Выход"]
    );
    assert_eq!(wm::about_title(), "О программе Rexon");
    assert_eq!(wm::forks_title(), "Установлены замены частей Plasma");
    assert_eq!(wm::forks_remove_label(), "🧹 Удалить их и вернуть обычную Plasma");
    assert_eq!(wm::forks_theme_label(), "Только сбросить тему");
    assert_eq!(wm::forks_cancel_label(), "Отмена");
    assert_eq!(wm::forks_blocked_title(), "Вернуть обычную Plasma сейчас нельзя");
    assert_eq!(wm::forks_confirm_title(), "Удалить замены частей Plasma?");
    assert_eq!(wm::forks_running_title(), "Удаление идёт");
    assert_eq!(
        wm::forks_running_text(),
        "Сейчас появится окно для ввода пароля. Когда удаление закончится, здесь появится итог."
    );
    assert_eq!(wm::forks_done_title(), "Замены удалены");
    assert_eq!(
        wm::forks_done_text(),
        "Пакеты-замены удалены, стандартные части Plasma возвращены.\n\n\
         Выйдите из сеанса и войдите снова, чтобы загрузилась обычная Plasma. \
         После входа при необходимости запустите «Восстановить тему KDE» ещё раз."
    );
    assert_eq!(wm::forks_failed_title(), "Не удалось удалить замены Plasma");
    assert_eq!(
        wm::forks_text(&["x".into()]),
        "Установлены пакеты <b>AeroThemePlasma / AeroShell</b>:<br><br>\
         &bull; <code>x</code><br><br>\
         Это <b>не тема оформления</b>: они заменяют сами части Plasma — \
         панель, окно запроса пароля, управление окнами. Пока они стоят, \
         <b>сброс темы их не уберёт</b>. Чтобы вернуть обычную Plasma, их \
         нужно удалить и поставить обратно стандартные пакеты."
    );
    assert_eq!(
        wm::forks_confirm_text("c"),
        "Будет выполнено (пароль спросят один раз):<br><br>\
         <code>c</code><br><br>\
         Займёт до минуты. После удаления нужно <b>выйти из сеанса и войти \
         снова</b>.<br>\
         Панель и виджеты в стиле Windows 7 (SevenStart) при этом пропадут."
    );
    assert_eq!(
        wm::recover_not_plasma_text("GNOME"),
        "Восстановление темы пока умеет только KDE Plasma.\n\
         Сейчас работает: GNOME. Ничего не изменено.\n\n\
         Здесь оформление возвращается настройками самого рабочего стола \
         либо во вкладке «Оформление»."
    );
    assert_eq!(wm::recover_failed_text("x"), "Не удалось запустить rexon: x");
    assert_eq!(wm::uninstall_text(), "Будут удалены файлы и директории Rexon");
    assert_eq!(
        wm::uninstall_always_line(),
        "При любом выборе удаляются сама программа (список выше) и временные файлы Rexon."
    );
    assert_eq!(
        wm::uninstall_delete_all_line(),
        "«Удалить всё» удалит также настройки Rexon, \
         резервные копии рабочих столов и установленные обои, темы и дополнения."
    );
    assert_eq!(
        wm::uninstall_delete_all_active_line(),
        "Если обои или тема экрана входа, \
         которые стоят сейчас, поставлены из Rexon, «Удалить всё» снимет и их — \
         рабочий стол вернётся к обычному виду."
    );
    assert_eq!(
        wm::uninstall_keep_hint(),
        "«Удалить, но сохранить настройки и резервные копии» \
         оставит на месте настройки, резервные копии и установленные обои, темы и дополнения: \
         если потом установите Rexon снова, он продолжит с того же места."
    );
    assert_eq!(wm::uninstall_keep_label(), "Удалить, но сохранить настройки и резервные копии");
    assert_eq!(wm::uninstall_delete_label(), "Удалить всё");
    assert_eq!(wm::uninstall_cancel_label(), "Отмена");
    assert_eq!(wm::uninstall_done_title(), "Удаление завершено");
    assert_eq!(wm::uninstall_done_text("Итог"), "Итог\n\nПриложение будет закрыто.");
    assert_eq!(wm::uninstall_failed_title(), "Ошибка удаления");
    use rexon_core::installer::UserDataKind::*;
    assert_eq!(
        wm::uninstall_data_paths_text(&[(Cache, "/c".into()), (Settings, "/s".into())]),
        "Временные файлы Rexon — удаляются при любом выборе\n  • /c\n\
         Настройки Rexon — только при «Удалить всё»\n  • /s"
    );
    let informative = wm::uninstall_informative(&[], &[(Cache, "/c".into())]);
    assert!(informative.contains("<p>Где это лежит:</p>"), "{informative}");
    let files: Vec<String> = (0..20).map(|i| format!("/f{i}")).collect();
    assert!(wm::uninstall_files_text(&files).ends_with("  • … ещё 5"));
    let one: Vec<String> = (0..16).map(|i| format!("/f{i}")).collect();
    assert!(wm::uninstall_files_text(&one).ends_with("  • … ещё 1"));
}

fn cyrillic(text: &str) -> bool {
    text.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё'))
}

fn english_texts() -> Vec<String> {
    use rexon_core::installer::UserDataKind::*;
    let mut all: Vec<String> = wm::menu_items().into_iter().map(|i| i.label).collect();
    all.extend([
        wm::about_title(),
        wm::about_html(),
        wm::about_credits_html(),
        wm::recover_title(),
        wm::recover_question("Breeze"),
        wm::recover_not_plasma_text("GNOME"),
        wm::recover_started_title(),
        wm::recover_started_text("Breeze"),
        wm::error_title(),
        wm::recover_failed_text("x"),
        wm::forks_text(&["x".into()]),
        wm::forks_confirm_text("c"),
        wm::uninstall_title(),
        wm::uninstall_text(),
        wm::uninstall_informative(
            &(0..20).map(|i| format!("/f{i}")).collect::<Vec<_>>(),
            &[(Cache, "/c".into()), (Settings, "/s".into()), (Backups, "/b".into()), (Content, "/w".into())],
        ),
        wm::uninstall_keep_label(),
        wm::uninstall_delete_label(),
        wm::uninstall_cancel_label(),
        wm::uninstall_done_title(),
        wm::uninstall_done_text("Done"),
        wm::uninstall_failed_title(),
    ]);
    let prompt = wm::forks_prompt(&["x".into()]).unwrap();
    all.push(prompt.title);
    all.extend(prompt.buttons.into_iter().map(|b| b.label));
    let step = wm::forks_preview_step(true, "c");
    all.extend([step.title, step.running_title, step.running_text]);
    all.push(wm::forks_preview_step(false, "why").title);
    let (t1, x1) = wm::forks_finished(true, "");
    let (t2, _) = wm::forks_finished(false, "why");
    all.extend([t1, x1, t2]);
    all
}

#[test]
fn english_menu_has_no_russian() {
    let _lang = i18n_pin::en();
    let all = english_texts();
    assert!(all.len() > 30, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!text.is_empty(), "пустой текст в английском меню");
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// Английский — витрина: пункты меню и кнопки в Title Case, атрибуция LGPL
/// на месте, кнопки названы в пояснениях дословно.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    let labels: Vec<String> = wm::menu_items().into_iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        ["🛟 Restore KDE Theme", "ℹ️ About Rexon", "🗑️ Uninstall Rexon", "🚪 Quit"]
    );
    let about = wm::about_html();
    for token in ["LGPL-3.0", "LGPL-2.1+", "dynamically linked", "KDE&nbsp;Frameworks", "Qt (LGPL-3.0)"] {
        assert!(about.contains(token), "из справки пропала атрибуция: {token}");
    }
    assert_eq!(wm::uninstall_cancel_label(), "Cancel");
    assert!(wm::uninstall_keep_hint().starts_with("“Uninstall but Keep Settings and Backups” keeps"));
    assert!(wm::forks_done_text().contains("run “Restore KDE Theme” again"));
    let files: Vec<String> = (0..16).map(|i| format!("/f{i}")).collect();
    assert!(wm::uninstall_files_text(&files).ends_with("  • … 1 more"));
}
