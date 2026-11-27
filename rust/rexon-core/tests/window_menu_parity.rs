//! Check of the main window MENU: the core `rexon_core::window_menu` (items,
//! «About», «Restore KDE theme», «Uninstall Rexon») and its markup in `rexon-gui`.
//!
//! WHAT THE REFERENCE IS HERE (since 02.10.2026). Python in Rexon is only an
//! archive: it is neither a reference nor a bridge. Formerly every core string
//! had to be present in the live `rexon/qt_main/window.py`; now the reference
//! is the current texts and structure of the core itself, written here
//! verbatim, and the skeleton snapshot `golden/qt_main_cases.json` (menu
//! items). Editing a core text turns the check red until the reference is
//! deliberately updated.
//!
//! WHAT THE CHECK DOES NOT MEASURE. The menu look: whether it opened, whether
//! the delegates were built, whether the keyboard moves through it. That is
//! measured by the window run (`rexon-main-window --selftest`, the
//! `REXON-MAINWINDOW-MENU*` lines) and by eye.

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

/// Remove everything that differs only in notation: spaces and line breaks.
fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/qt_main_cases.json")).unwrap()
}

// ============================================================================
// Menu contents
// ============================================================================

/// The «☰» button and four items in product order.
#[test]
fn menu_items_match_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::MENU_BUTTON, "☰");

    let items = wm::menu_items();
    assert_eq!(items.len(), 4, "в меню продукта ровно четыре пункта");
    let shortcuts: Vec<&str> = items.iter().map(|i| i.shortcut).collect();
    assert_eq!(shortcuts, ["", "", "", "Ctrl+Q"], "горячие клавиши пунктов меню");

    // The order is not a matter of taste: «Quit» is last, behind a separator,
    // because it is the only irreversible item without a question.
    let ids: Vec<&str> = items.iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        vec![wm::ID_RECOVER_KDE, wm::ID_ABOUT, wm::ID_UNINSTALL, wm::ID_QUIT],
        "порядок пунктов меню разошёлся с продуктом"
    );

    // With the skeleton snapshot too: a divergence here means that the core and
    // the skeleton reference describe different menus.
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

/// There is no second copy of «Quit» in the project: the tray builds the label with the icon.
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

/// There is exactly one separator, and it stands before «Quit» — as in `_build_menubar`.
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

/// The «☰» button stands on the RIGHT, and the submenu under it is laid out left to right.
///
/// The button's place is not cosmetic: the section bar starts right to its
/// left, and a button placed there too reads as the first section. The
/// decision lives in `qml/WindowMenu.qml`: the menu bar's `contentItem` is laid
/// out `Qt.RightToLeft`, and the submenu — a separate window — does not
/// inherit the bar's layout and stays left to right.
///
/// Right-to-left language: mirroring is turned on by the window ROOT
/// (`MainWindow.qml`, guard `rtl_parity.rs`), and with it the bar and the
/// submenu flip together — that is by design. What is forbidden here is a
/// LOCAL `LayoutMirroring` in `WindowMenu.qml` itself: it would flip the
/// submenu even with a left-to-right language.
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
        "в полосе меню включили местное зеркалирование — развернётся и подменю; \
         зеркалирование для языка справа налево ставится только на корне окна"
    );
}

/// There are no disabled items in the product — and they must not be invented.
#[test]
fn no_menu_item_is_disabled_in_the_product() {
    let _lang = i18n_pin::ru();
    for item in wm::menu_items() {
        assert!(wm::is_enabled(item.id), "пункт «{}» стал недоступен", item.id);
    }
}

// ============================================================================
// «About»
// ============================================================================

#[test]
fn about_text_matches_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::about_title(), "О программе Rexon");
    assert_eq!(
        wm::about_html(),
        format!(
            "<h2>Rexon {}</h2><p>Менеджер рабочих столов для Linux</p>\
             <p>Установка, удаление и настройка рабочих столов, тем, значков, курсоров, \
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

/// The capabilities paragraph was rewritten on 01.10.2026 from jargon («DE
/// setup… (incl. …) SDDM/Plymouth… — through a single interface») into plain
/// words: the same sections in the same order, without abbreviations.
#[test]
fn about_features_reworded_on_purpose() {
    let _lang = i18n_pin::ru();
    let html = wm::about_html();
    let want = "<p>Установка, удаление и настройка рабочих столов, тем, значков, курсоров, \
                шрифтов, обоев (включая Wallpaper&nbsp;Engine), экрана входа и загрузочной \
                заставки (SDDM/Plymouth), виджетов и расширений — всё в одном окне.</p>";
    assert!(html.contains(want), "{html}");
    let mut at = 0;
    for item in ["рабочих столов", "тем", "значков", "курсоров", "шрифтов", "обоев",
                 "Wallpaper&nbsp;Engine", "SDDM", "Plymouth", "виджетов", "расширений"] {
        let pos = want[at..].find(item).unwrap_or_else(|| panic!("пропал раздел «{item}»"));
        at += pos + item.len();
    }
    assert!(!html.contains(" DE,") && !html.contains("вкл."), "жаргон вернулся: {html}");
}

/// The LGPL attribution is legal text, not interface decoration: both
/// libraries are used via dynamic linking, and stating this is a condition of
/// their use when distributing Rexon. Losing this paragraph from the about
/// box is a licence violation, not a cosmetic issue.
#[test]
fn about_keeps_the_lgpl_attribution() {
    let _lang = i18n_pin::ru();
    let html = wm::about_html();
    for token in ["LGPL-3.0", "LGPL-2.1+", "динамическая", "KDE&nbsp;Frameworks"] {
        assert!(html.contains(token), "из справки пропала атрибуция: {token}");
    }
}

// ============================================================================
// «About»: credits versus NOTICE and README
// ============================================================================

/// NOTICE sections: (heading, text up to the next heading). A heading is a
/// line under which stands a rule made only of «-».
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

/// Whether a NOTICE section heading fits a table entry: a `notice_heading`
/// ending in «\n» is the whole heading, otherwise the start of the heading.
fn heading_matches(heading: &str, key: &str) -> bool {
    match key.strip_suffix('\n') {
        Some(exact) => heading == exact,
        None => heading.starts_with(key),
    }
}

/// NOTICE sections that are DELIBERATELY absent from the credits — with a reason.
/// A new NOTICE section without an entry here or in `THIRD_PARTY` turns the
/// check red: it means a foreign program appeared in the shipped build, and
/// the about box is silent about it.
const NOTICE_NOT_IN_ABOUT: &[(&str, &str)] = &[
    ("gemmology ", "часть fxtranslate, благодарность — fxtranslate"),
    ("xsimd ", "часть fxtranslate, благодарность — fxtranslate"),
    ("Noto Sans ", "подмножество шрифта для заставки, не программа"),
    ("Снимки рабочих столов", "картинки карусели, не программа"),
];

/// The credits table and NOTICE do not diverge: every table entry has a NOTICE
/// section with the same link and the same licence, and every NOTICE section
/// has an entry in the table (or a reason why it does not).
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

/// The same list stands in the README «Credits» section — the names did not diverge.
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

/// Credits in the about box: every program as a link and with a licence, and
/// the window body is the whole about text with the credits under it.
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

/// The disclaimer of affiliation with Wallpaper Engine and Valve stands in
/// «About» and matches the README verbatim: the «Wallpaper Engine» mark is in
/// the interface, and without the disclaimer it could be taken for a sign of
/// partnership. There is one string (`window_menu::not_affiliated_text`), no copy.
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

/// The version in the about box is the same as the crate's (the workspace
/// `version` in `rust/Cargo.toml`, the package takes it too): two copies of
/// the number would drift silently, and people report defects by the number
/// in the about box.
#[test]
fn about_version_matches_the_crate() {
    assert_eq!(
        wm::version_string(),
        env!("CARGO_PKG_VERSION"),
        "версия в справке (`constants::REXON_VERSION`) разошлась с версией крейта"
    );
}

// ============================================================================
// «Restore KDE theme»
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

/// The question ITSELF warns about wallpapers. A theme reset stops live and
/// video wallpapers; a person not warned about this will take the vanished
/// wallpaper for a Rexon breakage (rule #13).
#[test]
fn recover_question_warns_about_wallpapers() {
    let _lang = i18n_pin::ru();
    let q = wm::recover_question("Breeze");
    assert!(q.contains("обои сбросятся"), "вопрос молчит про обои: {q}");
    assert!(q.contains("Breeze"), "вопрос не назвал тему: {q}");
}

/// The reset command is the same CLI flag the product uses, but it is called
/// by the window's OWN binary (the route is entirely in Rust), not the Python entry.
#[test]
fn recover_uses_the_same_cli_flag() {
    assert_eq!(
        wm::recover_argv(Some("/x/rexon-main-window")),
        vec!["/x/rexon-main-window", "--recover-kde"]
    );
}

/// Before a theme reset the window checks packages replacing Plasma parts and
/// follows the core's steps: a window with three buttons, the next step by
/// button, the last question with the command, the outcome. The markup has no
/// words or decisions of its own.
#[test]
fn plasma_forks_steps_come_from_the_core() {
    let _lang = i18n_pin::ru();
    // The window asks the core for the next step.
    let qml = source("rust/rexon-gui/qml/WindowMenuDialogs.qml");
    assert!(qml.contains("menuModel.forksNext("), "Rust-окно решает шаг само");
    for id in [wm::FORKS_BUTTON_REMOVE, wm::FORKS_BUTTON_THEME, wm::FORKS_BUTTON_CANCEL] {
        assert!(qml.contains(&format!("forksChoice(\"{id}\")")), "кнопка «{id}» без шага");
    }
    // For the person — without jargon: no «forks», no «stock», no package manager
    // name in the window's words (the command in the question is a separate field).
    let prompt = wm::forks_prompt(&["x".into()]).unwrap();
    let mut words = vec![prompt.title, wm::forks_text(&["x".into()]), wm::forks_done_text()];
    words.extend(prompt.buttons.into_iter().map(|b| b.label));
    for text in &words {
        let lower = text.to_lowercase();
        for jargon in ["форк", "сток", "pacman"] {
            assert!(!lower.contains(jargon), "жаргон «{jargon}» в окне замен Plasma: {text}");
        }
    }
    // The meaning of the last question is the same: one password, a re-login is
    // needed, the Win7 panel will disappear.
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
    // A non-standard XDG takes the copy along — a hard-coded `~/.config` would
    // read the wrong file and name the wrong theme to the person.
    assert_eq!(
        wm::recover_backup_path("/home/кто-то", Some("/подстава/конфиг")),
        "/подстава/конфиг/rexon/last_kde_lookandfeel.txt"
    );
}

// ============================================================================
// «Uninstall Rexon»
// ============================================================================

#[test]
fn uninstall_dialog_texts_match_the_product() {
    let _lang = i18n_pin::ru();
    assert_eq!(wm::uninstall_title(), "Удалить Rexon");
    assert_eq!(wm::uninstall_text(), "Будут удалены файлы и директории Rexon");
    // Wehrwolfmann's decision of 15.09.2026: «keep» leaves the settings together
    // with the backups, and the core's words say so. The old labels («keep
    // backups» — while the settings were wiped) must not come back.
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
    // The path block is a wrapping <pre> (`ui_base::dialog_pre`): in a bare one a
    // long path went past the right edge of the window (snapshot of 30.09.2026).
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
            "<pre dir=\"ltr\" align=\"left\" style=\"white-space: pre-wrap\">  • /tmp/rexon</pre>\
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

/// The list is cut at the same number as in the product, and the remainder is counted.
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

/// The default button is «Cancel». Foolproofing: an accidental Enter must not
/// wipe the program off the buyer's machine. The core decides, the markup
/// gets the decision as the `defaultId` field.
#[test]
fn cancel_is_the_default_button() {
    assert_eq!(wm::UNINSTALL_DEFAULT_ID, "cancel");
    let bridge = source("rust/rexon-gui/src/window_menu.rs");
    assert!(
        bridge.contains("(\"defaultId\", qstr(wm::UNINSTALL_DEFAULT_ID))"),
        "окно удаления берёт кнопку по умолчанию мимо ядра"
    );
}

/// Uninstalling goes through the core: detecting the file owner, `pkexec
/// pacman -R` for a distro package and the path list. There is no second
/// removal engine in the project.
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
// Translation: Russian «as it was» and English
// ============================================================================
//
// The texts of the menu and its windows were hard-coded in Russian; now the
// source language is English, Russian lives in `po/ru.po`. Below — the whole
// Russian catalog (the uninstall window of 15-16.09, the remainder list),
// English — as a separate check.

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

/// English is the showcase: menu items and buttons in Title Case, the LGPL
/// attribution in place, buttons named verbatim in explanations.
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
