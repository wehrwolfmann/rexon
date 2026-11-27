//! Check of `qt_desktops.rs` — the «Desktops» tab.
//!
//! The golden here is strings and numbers written into the tests verbatim. Once they
//! were copied from the widget tab `rexon/qt_desktops/window.py`; since
//! 02.10.2026 Python in Rexon is only an archive, and these records are Rust's own
//! golden: any shift of them in the core turns the check red until the golden is corrected
//! deliberately.
//!
//! The REMOVAL PROTECTIONS are checked separately: removing the current desktop and removing
//! the last installed one is impossible under any input. This is what
//! costs a person their graphical session, so it is checked with a matrix, not
//! a single case (rule #13).
//!
//! Deliberate differences from the former widget tab:
//!   1. the hint «Not available on this distribution» is no longer swallowed
//!      when the availability probe throws an exception — the core has no probe,
//!      the answer is computed from a table;
//!   2. status colours are in hexadecimal notation (`#f59e0b` instead of
//!      `rgba(245,158,11,1)`): QML does not understand the QSS wrapper, and the colour is the same.

mod i18n_pin;

use rexon_core::appdefaults;
use rexon_core::qt_compare as qc;
use rexon_core::qt_desktops as qd;
use rexon_core::system::{self, PkgFamily, DE_META};

// ── The list on the left ────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn window_and_list_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::window_title(), "Rexon — Рабочие столы");
    assert_eq!(qd::WINDOW_MIN_SIZE, (960, 720));
    assert_eq!(qd::list_header(), "🖥️ Рабочие столы");
    assert_eq!(qd::LIST_HEADER_POINT_SIZE, 13);
    assert_eq!(qd::LIST_ITEM_POINT_SIZE, 12);
    assert_eq!(qd::SECTION_POINT_SIZE, 9);
    assert_eq!(qd::section_classic(), "Классические");
    assert_eq!(qd::section_tiling(), "Тайловые");
    assert_eq!(qd::section_effects(), "Композиторы с эффектами");
    assert_eq!(qd::placeholder_text(), "← Выберите рабочий стол из списка слева");
    assert_eq!(qd::compare_back_text(), "← Рабочие столы");
}

/// The contents of the «Tiling» section are duplicated in two places: the product choice
/// "whom to pull out and in which order" is here, while "is this a tiling compositor"
/// is decided by `appdefaults`. If they diverge, the buyer will see a new tiling
/// WM among the «Classic» ones, and this can only be noticed by eye.
///
/// The check goes strictly over desktops known to the core: for unknown ids
/// `is_tiling_wm` answers `true`, but there are no such ones in the database.
#[test]
fn tiling_section_matches_core() {
    for de_id in system::list_de_ids() {
        assert_eq!(
            qd::TILING_IDS.contains(&de_id),
            appdefaults::is_tiling_wm(de_id),
            "состав секции «Тайловые» разошёлся с ядром на столе «{de_id}»"
        );
    }
}

#[test]
fn tiling_order_is_fixed() {
    // Hyprland first as the most popular — the order is a product choice, the core does
    // not know it.
    assert_eq!(qd::TILING_IDS, &["hyprland", "niri", "sway", "river"]);
}

#[test]
fn item_text_is_icon_and_name() {
    // Two spaces between the icon and the name — as in the original
    // (`f"{icon}  {de.display_name}"`).
    assert_eq!(qd::item_text("kde"), format!("{}  KDE Plasma", qc::de_icon("kde")));
    assert_eq!(qd::item_text("xfce"), format!("{}  XFCE", qc::de_icon("xfce")));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn item_tooltip_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::item_tooltip("xfce", false, true), "");
    assert_eq!(qd::item_tooltip("xfce", true, true), "Установлен");
    assert_eq!(
        qd::item_tooltip("xfce", false, false),
        "Недоступен на этом дистрибутиве"
    );
    // COSMIC is the only desktop with its own hint: a hybrid.
    assert_eq!(
        qd::item_tooltip("cosmic", false, true),
        "Гибрид: рабочий стол с опциональным тайлингом"
    );
    assert_eq!(
        qd::item_tooltip("cosmic", true, false),
        "Установлен · Гибрид: рабочий стол с опциональным тайлингом · \
Недоступен на этом дистрибутиве"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn list_rows_split_into_three_sections() {
    let _lang = i18n_pin::ru();
    let rows = qd::list_rows(PkgFamily::Arch, &|_| false);
    let sections: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == "section")
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(sections, vec!["Классические", "Тайловые", "Композиторы с эффектами"]);

    // Tiling ones go in their own order and ONLY after the section header.
    let tiling_start = rows
        .iter()
        .position(|r| r.kind == "section" && r.text == "Тайловые")
        .expect("секция «Тайловые» на месте");
    let effects_start = rows
        .iter()
        .position(|r| r.kind == "section" && r.text == "Композиторы с эффектами")
        .expect("секция «Композиторы с эффектами» на месте");
    let ids = |range: &[qd::ListRow]| -> Vec<String> {
        range.iter().filter(|r| r.kind == "de").map(|r| r.de_id.clone()).collect()
    };
    assert_eq!(ids(&rows[tiling_start + 1..effects_start]), vec!["hyprland", "niri", "sway", "river"]);
    // Wayfire comes after «Tiling», in its own section: the windows are floating, but it is not
    // a full desktop.
    assert_eq!(ids(&rows[effects_start + 1..]), vec!["wayfire"]);

    // COSMIC stays among the «Classic» ones — it is a full-fledged hybrid.
    let classic = ids(&rows[..tiling_start]);
    assert!(classic.iter().any(|c| c == "cosmic"), "COSMIC уехал из «Классических»");
    assert!(!classic.iter().any(|c| c == "wayfire"), "Wayfire вернулся в «Классические»");

    // The list has exactly as many desktops as the database — not one is lost.
    let total = rows.iter().filter(|r| r.kind == "de").count();
    assert_eq!(total, DE_META.len());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn list_rows_carry_installed_flag() {
    let _lang = i18n_pin::ru();
    let rows = qd::list_rows(PkgFamily::Arch, &|id| id == "kde");
    let kde = rows.iter().find(|r| r.de_id == "kde").expect("KDE в списке");
    assert!(kde.installed);
    assert_eq!(kde.tooltip, "Установлен");
    let xfce = rows.iter().find(|r| r.de_id == "xfce").expect("XFCE в списке");
    assert!(!xfce.installed);
}

// ── The «Tools» panel ───────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn tool_buttons_match_python() {
    let _lang = i18n_pin::ru();
    // Where there is nothing to take a panel backup with — there are no panel buttons.
    let common = qd::tool_buttons("xfce", false);
    assert_eq!(common.len(), 2, "без снимка панели кнопок панели быть не должно");
    assert_eq!(common[0].kind, "compare");
    assert_eq!(common[0].text, "⚔️ Сравнение рабочих столов");
    assert_eq!(
        common[0].tooltip,
        "Таблица сравнения окружений, быстрые пары и рекомендации"
    );
    assert_eq!(common[1].kind, "repair_current");
    assert_eq!(common[1].text, "🔧 Проверить мой стол");
    assert_eq!(
        common[1].tooltip,
        "Проверить, что у текущего рабочего стола все пакеты на месте, \
и доустановить недостающие"
    );

    // KDE, MATE, XFCE — the same buttons with common labels, without «KDE».
    for de in ["kde", "mate", "xfce"] {
        let b = qd::tool_buttons(de, true);
        assert_eq!(b.len(), 4, "{de}: добавляются две кнопки панели");
        assert_eq!(b[2].kind, "panel_backup");
        assert_eq!(b[2].text, "🛟 Сохранить панель");
        assert_eq!(b[3].kind, "panel_restore");
        assert_eq!(b[3].text, "↩️ Вернуть панель");
        assert!(b[2].tooltip.starts_with("Сохранить копию настроек панели"), "{}", b[2].tooltip);
        assert!(!b[2].tooltip.contains("KDE") && !b[3].tooltip.contains("KDE"), "{de}");
    }
    assert!(!qd::panel_backup_result(true, "x").text.contains("KDE"));
    assert!(!qd::panel_restore_result(true, "x").text.contains("KDE"));
    // What takes the backup in which environment: the common copy spec of
    // every desktop (25.11.2026); XFCE no longer needs xfce4-panel-profiles.
    let have = |t: &str| ["plasmashell", "mate-panel", "xfce4-panel", "xfconf-query", "lxqt-panel"].contains(&t);
    for de in ["kde", "mate", "xfce", "lxqt", "cinnamon-mint"] {
        assert!(qd::panel_backup_supported(de, &have), "{de}");
    }
    assert!(!qd::panel_backup_supported("xfce", &|_| false), "без xfce4-panel нечем");
    assert!(!qd::panel_backup_supported("gnome", &have));
    assert_eq!(qd::tools_header(), "🧰 Инструменты");
    assert_eq!(qd::TOOL_BUTTON_MIN_HEIGHT, 40);
}

// ── Card ────────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn status_view_matches_python() {
    let _lang = i18n_pin::ru();
    let installed = qd::status_view(true, true);
    assert_eq!(installed.text, "✅ Установлен");
    assert_eq!(installed.color, "#4ade80");
    assert!(installed.bold);

    let missing = qd::status_view(false, true);
    assert_eq!(missing.text, "❌ Не установлен");
    // The original painted this line with ordinary secondary text.
    assert_eq!(missing.color, "");
    assert!(!missing.bold);

    let unavailable = qd::status_view(false, false);
    assert_eq!(unavailable.text, "⚠️ Недоступен");
    // The same colour as the original's `rgba(245,158,11,1)`, in QML notation.
    assert_eq!(unavailable.color, "#f59e0b");
    assert!(unavailable.bold);

    assert_eq!(qd::unavailable_notice(), "⚠️ Недоступен на вашем дистрибутиве");
    assert_eq!(qd::current_badge_text(), "✅ Текущий рабочий стол");
    assert_eq!(
        qd::current_badge_tooltip(),
        "Вы сейчас работаете в этом рабочем столе"
    );
    assert_eq!(qd::CURRENT_BADGE_COLOR, "#22c55e");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn stats_match_de_database() {
    let _lang = i18n_pin::ru();
    for meta in DE_META {
        let cells = qd::stats(meta.id);
        assert_eq!(cells.len(), 4, "у «{}» не четыре колонки ТТХ", meta.id);
        assert_eq!(cells[0].icon, "💾");
        assert_eq!(cells[0].value, format!("{} МБ", meta.ram_mb));
        assert_eq!(cells[0].label, "Память");
        assert_eq!(cells[1].icon, "⚡");
        assert_eq!(cells[1].value, meta.cpu_usage);
        assert_eq!(cells[1].label, "Процессор");
        // The rating — as in «Comparison»: icons by rating and «☆» up to five, so
        // that empty places are visible and the column is never empty.
        assert_eq!(cells[2].icon, rexon_core::qt_compare::stars(meta.customizable, "⭐"));
        assert_eq!(cells[2].icon.chars().count(), 5);
        assert_eq!(cells[2].label, "Настройка");
        assert_eq!(cells[3].icon, rexon_core::qt_compare::stars(meta.gaming_friendly, "🎮"));
        assert_eq!(cells[3].icon.chars().count(), 5);
        assert_eq!(cells[3].label, "Игры");
        // The original showed an empty value as a transparent dot.
        assert_eq!(cells[2].value, "");
        assert_eq!(cells[3].value, "");
    }
    assert_eq!(qd::STAT_EMPTY_VALUE, "·");
    assert!(qd::stats("нет-такого-стола").is_empty());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn description_drops_leading_emoji() {
    let _lang = i18n_pin::ru();
    // The card title already carries the icon: it is not needed a second time.
    for meta in DE_META {
        let text = qd::description(meta.id);
        let first = text.split(' ').next().unwrap_or("");
        assert!(
            first.chars().any(|c| c.is_alphanumeric()),
            "описание «{}» начинается со значка: {text}",
            meta.id
        );
    }
    // Cinnamon in the database starts with 🍃 — that is exactly what we strip.
    assert_eq!(
        qd::description("cinnamon-mint"),
        "Рабочий стол Cinnamon с полным набором компонентов Linux Mint"
    );
    // A description without an icon is not cut.
    assert_eq!(
        qd::description("xfce"),
        "Лёгкий и быстрый рабочий стол с низким потреблением ресурсов"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn packages_text_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qd::packages_text(12, 0),
        "📦 К установке 12 пакетов  (все из официальных репозиториев)"
    );
    assert_eq!(
        qd::packages_text(12, 3),
        "📦 К установке 15 пакетов  (12 из основных репозиториев + 3 из AUR)"
    );
}

/// The «packages» block — honestly by the desktop's state (08.10.2026: an installed
/// COSMIC showed «29 packages to install», for Pantheon names were counted rather than
/// the expanded group). The trial transaction's answer is fake.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn packages_view_is_honest() {
    use rexon_core::system_plan::InstallPreview;
    let _lang = i18n_pin::ru();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();

    // Not installed: the number is what will REALLY be installed; the group is expanded.
    let fresh = InstallPreview {
        groups: vec![("pantheon".into(), 62)],
        members: s(&["gala", "wingpanel", "lightdm"]),
        new_packages: s(&["gala", "wingpanel", "mutter", "libgee"]),
        skipped: s(&["switchboard-plug-online-accounts"]),
        ..InstallPreview::default()
    };
    let v = qd::packages_view(false, Some(&fresh), 3, 0);
    assert_eq!(
        v.text,
        "📦 К установке 4 пакета  (все из официальных репозиториев)\n\
         Группа «pantheon»: 62 пакета  ·  зависимости: 2  ·  уже установлено: 1"
    );
    assert!(v.notice.starts_with("⚠️ switchboard-plug-online-accounts сейчас не ставится"), "{}", v.notice);

    // Installed, everything in place: no "to install" at all.
    let full = InstallPreview { members: s(&["a", "b", "c"]), ..InstallPreview::default() };
    let v = qd::packages_view(true, Some(&full), 1, 0);
    assert_eq!(v.text, "✅ Все 3 пакета этого рабочего стола на месте");
    assert!(v.notice.is_empty());

    // Installed, part of it is missing.
    let gap = InstallPreview {
        members: s(&["a", "b", "c"]),
        new_packages: s(&["b", "c"]),
        ..InstallPreview::default()
    };
    let v = qd::packages_view(true, Some(&gap), 1, 0);
    assert_eq!(
        v.text,
        "⚠️ Не хватает 2 пакетов: b, c\n«Проверить / Восстановить» доставит недостающее."
    );

    // Installed, nothing missing, but new builds came out for some packages
    // (19.10.2026, MATE: mate-applets and mate-netbook) — this is not "missing".
    let outdated = InstallPreview {
        members: s(&["a", "mate-applets", "mate-netbook"]),
        updates: s(&["mate-applets", "mate-netbook"]),
        ..InstallPreview::default()
    };
    let v = qd::packages_view(true, Some(&outdated), 1, 0);
    assert_eq!(
        v.text,
        "✅ Все 3 пакета этого рабочего стола на месте\n\
         🔄 Для 2 пакетов есть обновление: mate-applets, mate-netbook — \
         оно придёт с обычным обновлением системы."
    );

    // Both missing packages and updates: "missing" means only the uninstalled ones.
    let both = InstallPreview {
        members: s(&["a", "b", "c"]),
        new_packages: s(&["b"]),
        updates: s(&["c"]),
        ..InstallPreview::default()
    };
    let v = qd::packages_view(true, Some(&both), 1, 0);
    assert_eq!(
        v.text,
        "⚠️ Не хватает 1 пакета: b\n«Проверить / Восстановить» доставит недостающее.\n\
         🔄 Для 1 пакета есть обновление: c — оно придёт с обычным обновлением системы."
    );

    // Without the probe's answer: for an installed one — the set's contents, not "to install".
    let v = qd::packages_view(true, None, 29, 0);
    assert_eq!(v.text, "📦 Набор рабочего стола: 29 пакетов");
    assert_eq!(
        qd::packages_view(false, None, 3, 0).text,
        "📦 К установке 3 пакета  (все из официальных репозиториев)"
    );

    // Something the desktop cannot exist without is broken — a refusal.
    let blocked = InstallPreview { blocked: s(&["pantheon-session"]), ..InstallPreview::default() };
    assert!(qd::packages_notice(&blocked).starts_with("⛔ Установка сейчас не пройдёт"));

    // The version line of an installed desktop.
    assert_eq!(qd::installed_version_line("1.0.3", "1.0.3", Some(false)), "Установлена версия 1.0.3");
    assert_eq!(
        qd::installed_version_line("1.0.3", "1.0.5", Some(true)),
        "Установлена версия 1.0.3, доступна 1.0.5"
    );
    assert_eq!(qd::installed_version_line("", "1.0.5", None), "");
}

/// A conflict of the set with an installed one (a live case of 19.10.2026: Deepin on
/// a machine with KDE Plasma — deepin-kwin versus kwin). The card and the refusal before
/// the password explain it in a human way; Rexon does not take it upon itself to remove another desktop.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn package_conflict_is_explained() {
    use rexon_core::system_plan::{conflict_explanation, InstallPreview, PkgConflict};
    let _lang = i18n_pin::ru();
    let c = |n: &str, i: &str| PkgConflict { new_package: n.into(), installed: i.into(), ..PkgConflict::default() };

    let deepin = InstallPreview { conflicts: vec![c("deepin-kwin", "kwin")], ..InstallPreview::default() };
    assert!(!deepin.installable());
    assert_eq!(
        qd::packages_notice(&deepin),
        "⛔ Deepin нельзя поставить рядом с KDE Plasma: оконный менеджер Deepin (deepin-kwin) \
         конфликтует с KWin. Чтобы поставить Deepin, придётся удалить KDE Plasma — сам Rexon \
         чужой рабочий стол не удаляет."
    );
    // Whose package it is — unknown: we name the packages, without made-up desktops.
    assert_eq!(
        conflict_explanation(&[c("kate", "kwrite")]),
        "⛔ Установка не пройдёт: kate конфликтует с установленным пакетом kwrite. \
         pacman пришлось бы удалить kwrite, а сам Rexon этого не делает."
    );
    // It does not matter who declares the conflict: the desktop is named after the installed one.
    assert_eq!(
        conflict_explanation(&[c("power-profiles-daemon", "tuned-ppd")]),
        "⛔ Установка не пройдёт: power-profiles-daemon конфликтует с установленным пакетом \
         tuned-ppd. pacman пришлось бы удалить tuned-ppd, а сам Rexon этого не делает."
    );
}

/// A live case of 20.10.2026 (MATE, Budgie next to it): Pantheon needs mutter49, and
/// mutter 51 is held by Budgie. We name the desktops and versions, not "pacman would have
/// to remove mutter"; not a desktop — we name who requires the package.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn package_conflict_names_the_holding_desktop() {
    use rexon_core::system_plan::{conflict_explanation, PkgConflict};
    let _lang = i18n_pin::ru();
    let mutter = PkgConflict {
        new_package: "mutter49".into(),
        installed: "mutter".into(),
        new_version: "49.8-1.1".into(),
        installed_version: "51.0-3".into(),
        same_program: true,
        new_desktops: vec!["Pantheon".into()],
        installed_desktops: vec!["Budgie".into()],
        required_by: vec!["budgie-desktop".into()],
        installed_is_new: false,
    };
    assert_eq!(
        conflict_explanation(&[mutter.clone()]),
        "⛔ Pantheon нельзя поставить рядом с Budgie: Pantheon нужен mutter 49 (mutter49), \
         а Budgie — mutter 51; оба сразу стоять не могут. Чтобы поставить Pantheon, придётся \
         удалить Budgie — сам Rexon чужой рабочий стол не удаляет."
    );
    let app = PkgConflict {
        installed_desktops: Vec::new(),
        required_by: vec!["some-app".into(), "other-app".into()],
        ..mutter
    };
    assert_eq!(
        conflict_explanation(&[app]),
        "⛔ Установка не пройдёт: mutter49 конфликтует с установленным пакетом mutter \
         (его требуют: some-app, other-app). pacman пришлось бы удалить mutter вместе с ними, \
         а сам Rexon этого не делает."
    );
}

/// A live defect of 15.11.2026: the Pantheon login screen pulls in mutter49, and mutter 51
/// is held by GNOME and Budgie. The desktop is installed without the login screen, «Install»
/// is active, and the text honestly says what will not be there and why.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn optional_conflict_is_explained_and_does_not_block() {
    use rexon_core::system_plan::{conflict_skip_explanation, ConflictSkip, InstallPreview, PkgConflict};
    let _lang = i18n_pin::ru();
    let mutter = PkgConflict {
        new_package: "mutter49".into(),
        installed: "mutter".into(),
        new_version: "49.8-1.1".into(),
        installed_version: "51.1-1".into(),
        same_program: true,
        new_desktops: vec!["Pantheon".into()],
        installed_desktops: vec!["GNOME".into(), "Budgie".into()],
        required_by: vec!["gnome-shell".into(), "budgie-desktop".into()],
        installed_is_new: false,
    };
    let skip = ConflictSkip { package: "lightdm-pantheon-greeter".into(), desktop: "Pantheon".into(), conflict: mutter.clone() };
    let want = "⚠️ Экран входа Pantheon не ставим: ему нужен Mutter 49, а GNOME, Budgie — Mutter 51; \
                оба сразу стоять не могут. Войти в Pantheon можно с обычного экрана входа, всё \
                остальное поставится.";
    assert_eq!(conflict_skip_explanation(std::slice::from_ref(&skip)), want);
    let p = InstallPreview { new_packages: vec!["gala".into()], conflict_skipped: vec![skip.clone()], ..Default::default() };
    let view = qd::packages_view(false, Some(&p), 0, 0);
    assert!(!view.install_blocked);
    assert_eq!(view.notice, want);
    // Not a login screen — we name the package.
    let app = ConflictSkip { package: "some-app".into(), ..skip };
    assert_eq!(
        conflict_skip_explanation(&[app]),
        "⚠️ some-app не ставим: ему нужен Mutter 49, а GNOME, Budgie — Mutter 51; оба сразу \
         стоять не могут. Всё остальное поставится без него."
    );
}

/// A conflict with an installed one (a live case of 20.10.2026: Deepin and Pantheon
/// next to Budgie/KDE): «Install» under the ⛔ text is visible but inactive, and
/// «Rebuild from Arch sources» is not offered — rebuilding the package does not
/// remove the conflict. Without a conflict everything is as before.
#[test]
fn conflict_disables_install_and_rebuild_offer() {
    use rexon_core::system_plan::{InstallPreview, PkgConflict};
    let conflict = InstallPreview {
        conflicts: vec![PkgConflict {
            new_package: "mutter49".into(),
            installed: "mutter".into(),
            ..PkgConflict::default()
        }],
        skipped: vec!["switchboard-plug-online-accounts".into()],
        ..InstallPreview::default()
    };
    assert!(qd::packages_view(false, Some(&conflict), 3, 0).install_blocked);
    assert!(qd::packages_view(true, Some(&conflict), 3, 0).install_blocked);
    assert!(!qd::rebuild_can_help(&conflict));

    let skipped_only = InstallPreview {
        skipped: vec!["switchboard-plug-online-accounts".into()],
        ..InstallPreview::default()
    };
    assert!(!qd::packages_view(false, Some(&skipped_only), 3, 0).install_blocked);
    assert!(qd::rebuild_can_help(&skipped_only));
    assert!(!qd::packages_view(false, None, 3, 0).install_blocked);

    // Exactly the buttons that install the desktop's set are inactive.
    let mut kinds: Vec<&str> = Vec::new();
    for (installed, current) in [(false, false), (true, false), (true, true)] {
        kinds.extend(qd::card_buttons(installed, true, current).iter().map(|b| b.kind));
    }
    kinds.sort_unstable();
    kinds.dedup();
    let off: Vec<&str> = kinds.iter().copied().filter(|k| qd::installs_desktop_set(k)).collect();
    assert_eq!(off, ["install", "repair"]);
}

/// Card buttons by the desktop's state — and with them the first protection.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn card_buttons_match_python() {
    let _lang = i18n_pin::ru();
    // Not available on the distribution — no buttons at all, only an explanation.
    assert!(qd::card_buttons(false, false, false).is_empty());
    assert!(qd::card_buttons(true, false, false).is_empty());

    // Not installed — one install button, highlighted.
    let fresh = qd::card_buttons(false, true, false);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].kind, "install");
    assert_eq!(fresh[0].text, "📦 Установить");
    assert_eq!(fresh[0].style, "primary");

    // Installed and NOT the current desktop — three buttons in the original's order.
    let other = qd::card_buttons(true, true, false);
    let kinds: Vec<&str> = other.iter().map(|b| b.kind).collect();
    assert_eq!(kinds, vec!["switch", "repair", "remove"]);
    assert_eq!(other[0].text, "🔄 Переключиться");
    assert_eq!(
        other[0].tooltip,
        "Переключиться на этот рабочий стол (настройки текущего сохранятся копией)"
    );
    assert_eq!(other[1].text, "🔧 Проверить / Восстановить");
    assert_eq!(
        other[1].tooltip,
        "Проверить, что все пакеты рабочего стола на месте, и доустановить недостающие"
    );
    assert_eq!(other[1].style, "neutral");
    assert_eq!(other[2].text, "🗑️ Удалить");
    assert_eq!(other[2].style, "destructive");
}

/// Card buttons go into the layout as a dictionary: the keys and kind values by
/// which the window chooses the handler and the style sheet are a closed set.
#[test]
fn card_buttons_serialize_with_stable_keys() {
    for (installed, available, current) in [(false, true, false), (true, true, false), (true, true, true)] {
        let value = serde_json::to_value(qd::card_buttons(installed, available, current)).unwrap();
        for button in value.as_array().unwrap() {
            let keys: Vec<&str> = button.as_object().unwrap().keys().map(String::as_str).collect();
            assert_eq!(keys, ["kind", "text", "tooltip", "style"]);
            assert!(["install", "switch", "repair", "remove"].contains(&button["kind"].as_str().unwrap()));
            assert!(["primary", "neutral", "destructive"].contains(&button["style"].as_str().unwrap()));
        }
    }
}

/// ⛔ The first protection: the CURRENT desktop has no «Switch» and «Remove» buttons
/// at all. There is nothing to press — so one cannot end up without graphics either.
#[test]
fn current_desktop_has_no_switch_and_no_remove() {
    let current = qd::card_buttons(true, true, true);
    let kinds: Vec<&str> = current.iter().map(|b| b.kind).collect();
    assert_eq!(kinds, vec!["repair"], "у текущего стола лишние кнопки: {kinds:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn version_line_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::version_checking(), "Проверяю версию…");
    assert_eq!(
        qd::version_line("6.1.2-1"),
        "Доступно в вашем дистрибутиве: 6.1.2-1"
    );
    // No version — the line is hidden rather than showing emptiness.
    assert_eq!(qd::version_line(""), "");
}

// ── Carousel ────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn carousel_texts_and_dots_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::no_screenshots_text(), "📷  Нет скриншотов");
    assert_eq!(qd::carousel_dots(0, 1), "", "при одном снимке точек нет");
    assert_eq!(qd::carousel_dots(0, 3), "●  ○  ○");
    assert_eq!(qd::carousel_dots(2, 3), "○  ○  ●");
    assert_eq!(
        qd::zoom_hint(0, 0),
        "× в углу, клик или Esc — закрыть"
    );
    assert_eq!(
        qd::zoom_hint(0, 1),
        "× в углу, клик по картинке или Esc — закрыть"
    );
    assert_eq!(
        qd::zoom_hint(1, 3),
        "× в углу, клик по картинке или Esc — закрыть · ←/→ листать  ·  2/3"
    );
}

#[test]
fn carousel_height_matches_python_formula() {
    // A small window: the lower bound applies.
    assert_eq!(qd::carousel_height(600, 400), 320);
    // A maximized Full HD: 55% of the window, but not above 720.
    assert_eq!(qd::carousel_height(1080, 1600), 594);
    assert_eq!(qd::carousel_height(2000, 3000), 720);
    // A narrow column: the ideal 16:9 is below the ceiling.
    assert_eq!(qd::carousel_height(1080, 700), 393);
}

/// ⭐ The screenshot frame gives height to the card when the window does not have enough of it.
///
/// WHAT IT CATCHES. On the standard 1100×750 window the status footer (34 points) cut off
/// the bottom row of the card's buttons: the viewport became 642 points, while the card
/// needs 666, and hyprland and niri need 687. The frame meanwhile stood at its floor of
/// 320 and did not yield a single point, because 16:9 of its width (553 points)
/// gives 311 — less than the floor. The guard holds both the new behaviour and both of its
/// bounds: without need the frame does not shrink, and it does not go below the limit.
#[test]
fn carousel_frame_yields_height_to_card_buttons() {
    // Measurement of 19.09.2026, the standard 1100×750 window, a card 569 wide.
    let full = qd::carousel_height(676, 569 - 2 * qd::CAROUSEL_FRAME_PADDING_H);
    assert_eq!(full, qd::CAROUSEL_MIN_HEIGHT, "рамка стоит на своём поле");

    // Enough height — the frame is not touched.
    assert_eq!(qd::carousel_height_fitted(full, 330), full);
    assert_eq!(qd::carousel_height_fitted(full, full), full);

    // The status footer took 34 points: the frame is left with 296, and that is what it
    // gives — exactly as much as is missing, no more.
    assert_eq!(qd::carousel_height_fitted(full, 296), 296);
    // The worst of the eleven cards (hyprland, niri): the description is a line
    // longer, the frame is left with 275.
    assert_eq!(qd::carousel_height_fitted(full, 275), 275);

    // The limit: beyond it the frame does not yield, even if asked for more.
    assert_eq!(qd::CAROUSEL_MAX_SQUEEZE, 72);
    let floor = full - qd::CAROUSEL_MAX_SQUEEZE;
    assert_eq!(qd::carousel_height_fitted(full, floor - 1), floor);
    assert_eq!(qd::carousel_height_fitted(full, 0), floor);
    assert_eq!(qd::carousel_height_fitted(full, -100), floor);
    assert!(floor > 0, "рамка не имеет права исчезнуть: {floor}");

    // The layout asks the core rather than computing by itself, and takes the frame width from the
    // UNshrunk height: compute it from the shrunk one — and you get a "ratchet".
    const QML: &str = include_str!("../../rexon-gui/qml/DesktopsPage.qml");
    assert!(
        QML.contains("page.carouselHeightFitted(frame.fullHeight, frame.room)"),
        "высота рамки снова считается мимо ядра"
    );
    assert!(
        QML.contains("frame.fullHeight * 16 / 9 + frame.sidePadding"),
        "ширина рамки больше не берётся от неужатой высоты"
    );
    assert!(
        !QML.contains("frame.height * 16 / 9"),
        "ширина рамки снова следует за ужатой высотой — это «храповик»"
    );
}

/// ⭐ «Enlarge to full screen» opens a full-screen WINDOW.
///
/// WHAT IT CATCHES. The screenshot enlargement was drawn by a tab-sized `Popup`:
/// the section bar and the status footer stayed over the picture, and the margins
/// around it were the page margins — i.e. the button label promised one thing and
/// showed another. In the widget original this is a frameless, modal `QDialog`
/// with `showFullScreen()` (`rexon/qt_desktops/window.py`).
///
/// The guard reads the layout and holds all four conditions: a top-level
/// window, full screen with a fallback size by the screen, modality and exit
/// by Esc. Esc is named separately: for a `Popup` it was handled by `closePolicy`, a
/// window has no such property, and during the move the key would have silently disappeared.
#[test]
fn screenshot_zoom_opens_a_real_fullscreen_window() {
    const QML: &str = include_str!("../../rexon-gui/qml/DesktopsPage.qml");

    assert!(QML.contains("    Window {\n        id: zoom\n"), "просмотр — не окно верхнего уровня");
    assert!(
        !QML.contains("Popup {\n        id: zoom"),
        "просмотр снова всплывашка внутри вкладки"
    );
    assert!(QML.contains("zoom.showFullScreen();"), "просмотр не просит полного экрана");
    assert!(QML.contains("flags: Qt.Window | Qt.FramelessWindowHint"), "у просмотра рамка окна");
    assert!(QML.contains("modality: Qt.ApplicationModal"), "просмотр не модальный");

    // The fallback size: `showFullScreen()` is a request to the window manager, and
    // where it is not fulfilled the window would stay the size of its
    // contents (measured on a bench without a window manager: 160×150).
    assert!(QML.contains("zoom.width = Screen.width;"), "нет запасного размера по экрану");
    assert!(QML.contains("zoom.height = Screen.height;"), "нет запасного размера по экрану");

    // Exit by Esc and one exit point for all four paths: a click on
    // the screenshot, the cross, Esc and Q.
    assert!(QML.contains("case Qt.Key_Escape:"), "Esc больше не закрывает просмотр");
    assert_eq!(
        QML.matches("zoom.hideZoom()").count(),
        3,
        "у просмотра снова несколько разных выходов"
    );
    assert_eq!(
        QML.matches("zoom.close()").count(),
        1,
        "окно закрывается мимо своей единственной точки выхода"
    );
}

// ── Operation texts ─────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn install_texts_match_python() {
    let _lang = i18n_pin::ru();
    let t = qd::install_texts("KDE Plasma");
    assert_eq!(t.confirm_title, "Установить?");
    assert_eq!(
        t.confirm_html,
        "Установить рабочий стол <b>KDE Plasma</b>?<br><br>\
Rexon попросит пароль администратора."
    );
    assert_eq!(t.operation_title, "Установка KDE Plasma");
    assert_eq!(t.done_title, "Установка завершена");
    // The usual result is logging out of the session: the desktop does not need a reboot (the live
    // River install on 24.10.2026 asked to reboot for nothing).
    assert_eq!(
        t.done_text,
        "<b>KDE Plasma</b> установлен.\n\
Выйдите из сеанса и выберите KDE Plasma на экране входа."
    );
    // A reboot only when the executor decided so from the install plan
    // (a kernel, a kernel module, a login screen enabled for the first time).
    assert_eq!(
        t.done_text_restart,
        "<b>KDE Plasma</b> установлен.\n\
Перезагрузите компьютер и выберите этот рабочий стол на экране входа."
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn repair_texts_match_python() {
    let _lang = i18n_pin::ru();
    let t = qd::repair_texts("XFCE");
    assert_eq!(t.confirm_title, "Проверить и восстановить?");
    assert_eq!(
        t.confirm_html,
        "Проверить и доустановить недостающие пакеты <b>XFCE</b>? \
Уже установленные пакеты будут пропущены.<br><br>Rexon попросит пароль администратора."
    );
    assert_eq!(t.operation_title, "Восстановление XFCE");
    assert_eq!(t.done_title, "Проверка завершена");
    assert_eq!(
        t.done_text,
        "Установка <b>XFCE</b> проверена.\n\
Недостающие пакеты (если были) доустановлены, уже установленные пропущены."
    );
    assert_eq!(
        t.done_text_restart,
        "Установка <b>XFCE</b> проверена.\n\
Недостающие пакеты (если были) доустановлены, уже установленные пропущены.\n\
Перезагрузите компьютер, чтобы изменения вступили в силу."
    );
}

/// The switch texts of both tabs are literally the same. The «Desktops» tab
/// does not keep its own copy — it takes `qt_compare::switch_texts`; this test
/// guards that they stayed there.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn switch_texts_are_shared_with_compare_tab() {
    let _lang = i18n_pin::ru();
    let t = qc::switch_texts("GNOME");
    assert_eq!(t.confirm_title, "Переключиться?");
    assert_eq!(
        t.confirm_text,
        "Переключиться на <b>GNOME</b>?\n\n\
Rexon сохранит копию настроек текущего рабочего стола и выйдет из сеанса."
    );
    assert_eq!(t.operation_title, "Переключение на GNOME");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn repair_current_unknown_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::repair_current_unknown_title(), "Рабочий стол не определён");
    assert_eq!(
        qd::repair_current_unknown_text(),
        "Не удалось определить текущий рабочий стол — выберите его в списке слева \
и нажмите «Проверить / Восстановить» на карточке."
    );
}

// ── ⛔ Removal protections ──────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn remove_current_desktop_is_blocked() {
    let _lang = i18n_pin::ru();
    let plan = qd::remove_plan("kde", "KDE Plasma", "kde", 5);
    assert_eq!(plan.kind, "blocked_current");
    assert!(!plan.allowed(), "удаление текущего стола нельзя разрешать");
    assert_eq!(plan.title, "Нельзя удалить текущий рабочий стол");
    assert_eq!(
        plan.text,
        "<b>KDE Plasma</b> — это ваш текущий рабочий стол.\n\n\
Если удалить его сейчас, после следующего входа вы останетесь без \
рабочего стола (чёрный экран).\n\n\
Что нужно сделать:\n\
1. Сначала переключитесь на другой установленный рабочий стол.\n\
2. Войдите в него.\n\
3. Только тогда удаляйте этот."
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn remove_last_desktop_is_blocked() {
    let _lang = i18n_pin::ru();
    let plan = qd::remove_plan("xfce", "XFCE", "kde", 0);
    assert_eq!(plan.kind, "blocked_last");
    assert!(!plan.allowed());
    assert_eq!(plan.title, "Это последний установленный рабочий стол");
    assert_eq!(
        plan.text,
        "<b>XFCE</b> — единственный установленный рабочий стол в системе.\n\n\
Если удалить его, после следующего выхода из сеанса или перезагрузки вы \
останетесь без рабочего стола (чёрный экран).\n\n\
Сначала установите другой рабочий стол, потом удаляйте этот."
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn remove_other_desktop_asks_confirmation() {
    let _lang = i18n_pin::ru();
    let plan = qd::remove_plan("xfce", "XFCE", "kde", 2);
    assert_eq!(plan.kind, "confirm");
    assert!(plan.allowed());
    assert_eq!(plan.title, "Удалить?");
    assert_eq!(
        plan.text,
        "Удалить рабочий стол <b>XFCE</b>?\n\n\
Текущий рабочий стол: <b>KDE Plasma</b> — он не пострадает.\n\
Все пакеты этого рабочего стола будут удалены. Rexon попросит пароль \
администратора."
    );
    assert_eq!(qd::remove_operation_title("XFCE"), "Удаление XFCE");
}

/// The matrix of both protections over ALL desktops of the database: removal is allowed exactly
/// when the desktop is not the current one and there is at least one other installed one.
#[test]
fn remove_guards_hold_for_every_desktop() {
    for meta in DE_META {
        for current in ["kde", "xfce", "", meta.id] {
            for others in [0_usize, 1, 7] {
                let plan = qd::remove_plan(meta.id, meta.display_name, current, others);
                let expected = meta.id != current && others > 0;
                assert_eq!(
                    plan.allowed(),
                    expected,
                    "стол «{}», текущий «{current}», других установлено {others}",
                    meta.id
                );
                // A refusal ALWAYS explains what to do: a silent button
                // here is worse than a refusal.
                if !plan.allowed() {
                    assert!(!plan.text.is_empty());
                    assert!(!plan.title.is_empty());
                }
            }
        }
    }
}

// ── Panel backup (KDE, MATE, XFCE) ─────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn panel_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::panel_restore_confirm_title(), "Вернуть панель?");
    // The question itself is the core's common one since 25.11.2026: it names the
    // desktop and the copy's date (`qt_settings::panel_restore_confirm`).

    let ok = qd::panel_backup_result(true, "снимок 2026-08-23");
    assert_eq!(ok.title, "Панель сохранена");
    assert_eq!(
        ok.text,
        "Копия настроек панели сохранена.\nснимок 2026-08-23"
    );

    let fail = qd::panel_backup_result(false, "нет места");
    assert_eq!(fail.title, "Не удалось сохранить");
    assert_eq!(fail.text, "Копия панели не сохранилась: нет места");

    let restored = qd::panel_restore_result(true, "из копии 2026-08-22");
    assert_eq!(restored.title, "Панель восстановлена");
    assert_eq!(
        restored.text,
        "Панель возвращена из копии.\nиз копии 2026-08-22"
    );

    let restore_fail = qd::panel_restore_result(false, "копий нет");
    assert_eq!(restore_fail.title, "Не удалось восстановить");
    assert_eq!(restore_fail.text, "Восстановление не выполнено: копий нет");
}

// ── Common guards ───────────────────────────────────────────────────────

/// Every desktop of the database has a name and a description: a card without them would look like
/// a product breakage, not like "no data".
#[test]
fn every_desktop_has_name_and_description() {
    for meta in DE_META {
        assert!(!qd::display_name(meta.id).is_empty(), "{}", meta.id);
        assert!(!qd::description(meta.id).is_empty(), "{}", meta.id);
    }
    // An unknown desktop is shown by its identifier, not by emptiness.
    assert_eq!(qd::display_name("нет-такого"), "нет-такого");
}

/// The «Available in your distribution» line must be requested when
/// the page gets the side-actions object.
///
/// The host sets it as a property only AFTER the layout has been built: in
/// `Component.onCompleted` it is still null, and a version request from there goes into
/// nowhere. Without the `onActionsChanged` handler the line never appeared
/// — the tab silently lost the package manager's answer. The guard reads the
/// layout itself: this cannot be checked on the core's texts.
#[test]
fn version_is_requested_when_actions_arrive() {
    let qml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/qml/DesktopsPage.qml"
    );
    let qml = std::fs::read_to_string(qml_path).expect("нет разметки DesktopsPage.qml");
    // The handler asks for both the version and the package set (the trial transaction
    // goes through the same actions object and gets lost without it the same way).
    let handler = qml
        .split_once("onActionsChanged: {")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(body, _)| body)
        .unwrap_or_default();
    assert!(
        handler.contains("root.requestPackages();"),
        "DesktopsPage.qml не спрашивает состав пакетов с появлением объекта действий"
    );
    assert!(
        handler.contains("root.requestVersion();"),
        "DesktopsPage.qml снова спрашивает версию только из Component.onCompleted — \
         объекта действий там ещё нет, и строка версии не появится никогда"
    );
}

// ── Layout colours and sizes ────────────────────────────────────────────

/// The numbers of the original's style sheet, written here verbatim: QML is painted
/// with them via the core, and any shift in the core turns the check red.
#[test]
fn layout_numbers_match_python() {
    assert_eq!(qd::LIST_MARGINS, (12, 16, 8, 12));
    assert_eq!(qd::LIST_SPACING, 8);
    assert_eq!((qd::LIST_MIN_WIDTH, qd::LIST_MAX_WIDTH, qd::LIST_INITIAL_WIDTH), (200, 280, 240));
    assert_eq!((qd::LIST_ITEM_PADDING_V, qd::LIST_ITEM_PADDING_H, qd::LIST_ITEM_RADIUS), (8, 12, 4));
    assert_eq!(qd::LIST_SELECTED_QSS, "rgba(59,130,246,0.4)");
    assert_eq!(qd::LIST_SELECTED_TEXT_COLOR, "#ffffff");
    assert_eq!(qd::list_hover_qss(true), "rgba(255,255,255,0.05)");
    assert_eq!(qd::list_hover_qss(false), "rgba(0,0,0,0.05)");

    assert_eq!(qd::TOOLS_MARGINS, (8, 16, 8, 12));
    assert_eq!((qd::TOOLS_SPACING, qd::TOOLS_MIN_WIDTH, qd::TOOLS_INITIAL_WIDTH), (8, 160, 170));
    assert_eq!(qd::TOOL_BUTTON_MAX_WIDTH, 240);
    assert_eq!(qd::TOOLS_PANEL_BG_ALPHA, 0.05);
    assert_eq!(qd::TOOLS_PANEL_BORDER_ALPHA, 0.18);

    assert_eq!((qd::CARD_MARGIN_TOP, qd::CARD_MARGIN_RIGHT, qd::CARD_MARGIN_BOTTOM), (20, 24, 24));
    assert_eq!((qd::CARD_SPACING, qd::HEADER_ROW_SPACING), (14, 12));
    assert_eq!(qd::DETAIL_SCROLLBAR_RESERVE, 20);
    assert_eq!(qd::SEPARATOR_ALPHA, 0.25);
    assert_eq!(qd::PLACEHOLDER_POINT_SIZE, 14);
    assert_eq!(qd::CARD_DESC_POINT_SIZE, 11);
    assert_eq!((qd::STAT_LABEL_POINT_SIZE, qd::STAT_SPACING), (9, 2));
    assert_eq!((qd::PACKAGES_POINT_SIZE, qd::VERSION_POINT_SIZE), (10, 10));
    assert_eq!((qd::BUTTON_ROW_TOP_MARGIN, qd::BUTTON_ROW_SPACING), (8, 8));
    assert_eq!((qd::CARD_BUTTON_RADIUS, qd::CARD_BUTTON_PADDING_V, qd::CARD_BUTTON_PADDING_H), (6, 8, 12));
    assert_eq!(qd::PRIMARY_BUTTON_QSS, "rgba(59,130,246,0.85)");
    assert_eq!(qd::PRIMARY_BUTTON_HOVER_QSS, "rgba(59,130,246,1.0)");
    assert_eq!(qd::DESTRUCTIVE_BUTTON_QSS, "rgba(220,80,80,0.75)");
    assert_eq!(qd::DESTRUCTIVE_BUTTON_HOVER_QSS, "rgba(220,80,80,1.0)");
    assert_eq!(qd::ACCENT_BUTTON_TEXT_COLOR, "#ffffff");
    assert_eq!(qd::NEUTRAL_BUTTON_BG_ALPHA, 0.10);
    assert_eq!(qd::NEUTRAL_BUTTON_BORDER_ALPHA, 0.30);
    assert_eq!(qd::NEUTRAL_BUTTON_HOVER_ALPHA, 0.20);

    assert_eq!(qd::CAROUSEL_FRAME_ALPHA, 0.10);
    assert_eq!((qd::CAROUSEL_FRAME_RADIUS, qd::CAROUSEL_FRAME_PADDING_H, qd::CAROUSEL_DOTS_SPACING), (8, 8, 6));
    assert_eq!(qd::NO_SCREENSHOTS_POINT_SIZE, 20);
    assert_eq!(qd::OVERLAY_BUTTON_QSS, "rgba(0,0,0,0.55)");
    assert_eq!(qd::ARROW_HOVER_QSS, "rgba(0,0,0,0.75)");
    assert_eq!(qd::ZOOM_BUTTON_HOVER_QSS, "rgba(0,0,0,0.85)");
    assert_eq!((qd::OVERLAY_MARGIN, qd::ARROW_SIZE, qd::ARROW_POINT_SIZE), (10, 40, 16));
    assert_eq!((qd::ZOOM_BUTTON_SIZE, qd::ZOOM_BUTTON_POINT_SIZE), (44, 20));
    // Roundings in QSS: 20 for the arrow, 22 for the magnifier, 24 for the view buttons.
    assert_eq!(qd::round_radius(qd::ARROW_SIZE), 20);
    assert_eq!(qd::round_radius(qd::ZOOM_BUTTON_SIZE), 22);
    assert_eq!(qd::round_radius(qd::ZOOM_ARROW_SIZE), 24);
    assert_eq!(qd::ZOOM_BG_QSS, "rgba(0,0,0,0.95)");
    assert_eq!((qd::ZOOM_MARGIN, qd::ZOOM_ARROW_SIZE, qd::ZOOM_ARROW_POINT_SIZE), (16, 48, 18));
    assert_eq!(qd::ZOOM_CLOSE_POINT_SIZE, 26);
    assert_eq!(qd::ZOOM_HINT_QSS, "rgba(255,255,255,0.6)");
    assert_eq!(qd::ZOOM_HINT_PADDING, 8);
}

/// The right panel's ceiling is the original's `_sync_detail_width` formula:
/// `h = max(320, min(0.55·window, 720))`, `ideal_w = h·16/9 + 16`,
/// ceiling = 12 + ideal_w + 24 + 20.
#[test]
fn detail_panel_max_width_matches_python_formula() {
    for (window_h, want) in [(600, 12 + (330 * 16 / 9 + 16) + 44), (400, 12 + (320 * 16 / 9 + 16) + 44), (2000, 12 + (720 * 16 / 9 + 16) + 44)] {
        assert_eq!(qd::detail_panel_max_width(window_h), want, "окно {window_h}");
    }
    assert_eq!(qd::detail_panel_max_width(6200), 1352);
}

/// QSS colours are converted to QML notation without loss: what the page is painted
/// with and what was copied from the original are one and the same colour.
#[test]
fn qss_colors_translate_for_qml() {
    use rexon_core::ui_base::{css_color_to_qml, neutral_rgba};
    // ⚠️ Fractions are truncated, the way Qt's style sheet parser truncates them: 0.4 → 0x66,
    // 0.85 → 0xd8, 0.75 → 0xbf, 0.55 → 0x8c, 0.6 → 0x99, 0.25 → 0x3f,
    // 0.05 → 0x0c. Rounding gave a colour one step denser than the widget one.
    assert_eq!(css_color_to_qml(qd::LIST_SELECTED_QSS), "#663b82f6");
    assert_eq!(css_color_to_qml(qd::PRIMARY_BUTTON_QSS), "#d83b82f6");
    assert_eq!(css_color_to_qml(qd::PRIMARY_BUTTON_HOVER_QSS), "#ff3b82f6");
    assert_eq!(css_color_to_qml(qd::DESTRUCTIVE_BUTTON_QSS), "#bfdc5050");
    assert_eq!(css_color_to_qml(qd::OVERLAY_BUTTON_QSS), "#8c000000");
    assert_eq!(css_color_to_qml(qd::ZOOM_HINT_QSS), "#99ffffff");
    assert_eq!(css_color_to_qml(&neutral_rgba(qd::SEPARATOR_ALPHA)), "#3f7f7f7f");
    assert_eq!(css_color_to_qml(&neutral_rgba(qd::TOOLS_PANEL_BG_ALPHA)), "#0c7f7f7f");
}

/// The section header colour: the skin's PlaceholderText role, and for the system skin
/// — secondary text with the palette plan's transparency.
#[test]
fn placeholder_text_color_follows_skin() {
    use rexon_core::ui_base::placeholder_text_color;
    assert_eq!(placeholder_text_color("steam_dark"), "#9fb4c6");
    assert_eq!(placeholder_text_color("light"), "#7a8086");
    assert_eq!(placeholder_text_color("system"), "#beffffff");
}

/// Splitting the window width between the panels — exactly as the original's QSplitter does.
///
/// ⚠️ THE NUMBERS ARE NOT DERIVED BUT MEASURED. Measurement of 05.09: the widget tab was built
/// seventeen times at different window widths (height 6400, skin steam_dark), and
/// after showing `QSplitter.sizes()` was read. Both corrections to the former
/// formula come from there: the handles take their share BEFORE splitting, and the extra space is split
/// proportionally to the starting widths 170:800, not by the 1:4 stretch. The former
/// formula was off by 8 points on the bench window, and the whole panel together with
/// the splitter moved to the right.
#[test]
fn split_widths_follow_splitter_stretch() {
    assert_eq!(qd::DETAIL_INITIAL_WIDTH, 800);
    assert_eq!((qd::SPLIT_STRETCH_TOOLS, qd::SPLIT_STRETCH_DETAIL), (1, 4));
    assert_eq!((qd::SPLIT_HANDLE_WIDTH, qd::SPLIT_HANDLE_COUNT), (1, 2));

    // Measured on the original: (window width, list, tools, card).
    let measured = [
        (960, 240, 160, 558),
        (1000, 240, 160, 598),
        (1100, 240, 160, 698),
        (1200, 240, 168, 790),
        (1300, 240, 185, 873),
        (1400, 240, 203, 955),
        (1500, 240, 220, 1038),
        (1600, 240, 238, 1120),
        (1700, 240, 256, 1202),
        (1800, 240, 273, 1285),
        (1900, 240, 306, 1352),
        (2000, 240, 406, 1352),
        (2100, 240, 506, 1352),
        (2200, 240, 606, 1352),
        (2400, 240, 806, 1352),
        (2600, 240, 1006, 1352),
        (3000, 240, 1406, 1352),
    ];
    for (total, list, tools, detail) in measured {
        assert_eq!(
            qd::split_widths(total, 6400),
            (list, tools, detail),
            "окно {total}"
        );
        // Panels plus handles are exactly the window width, without gaps or overlap.
        assert_eq!(
            list + tools + detail + qd::SPLIT_HANDLE_WIDTH * qd::SPLIT_HANDLE_COUNT,
            total,
            "окно {total}: панели не сошлись с шириной окна"
        );
    }

    // The minimum panel width is not broken even on the narrowest window.
    assert!(qd::split_widths(960, 6400).1 >= qd::TOOLS_MIN_WIDTH);
    // The card's ceiling stays at `detail_panel_max_width` at any height.
    let (list, tools, detail) = qd::split_widths(2400, 1080);
    assert_eq!(list, 240);
    assert_eq!(detail, qd::detail_panel_max_width(1080));
    assert_eq!(
        list + tools + detail + qd::SPLIT_HANDLE_WIDTH * qd::SPLIT_HANDLE_COUNT,
        2400
    );
}

/// The floor of the MIDDLE panel (since 25.10 — «Distro looks», formerly
/// «Tools») is computed from its contents, not from one number:
/// the labels must be readable in full at the window's standard minimum.
///
/// ⚠️ THE NUMBERS ARE MEASURED, NOT DERIVED. Measurement of 18.09 on the bench (skin steam_dark,
/// window 1100×750): in the widget original the splitter gave the panel exactly 160
/// points, while Qt itself computed its minimum size from the contents —
/// `tools.minimumSizeHint().width() == 165` with the longest button at 149.
#[test]
fn looks_panel_min_width_follows_its_content() {
    // The original's contents: 149 plus margins 8+8 — exactly its 165.
    assert_eq!(qd::looks_panel_min_width(142, 149), 165);
    // Longer: 216 plus the same margins.
    assert_eq!(qd::looks_panel_min_width(142, 216), 232);
    // The original's floor is not broken even if there is nothing to measure.
    assert_eq!(qd::looks_panel_min_width(0, 0), qd::TOOLS_MIN_WIDTH);
    assert_eq!(qd::looks_panel_min_width(-5, -5), qd::TOOLS_MIN_WIDTH);
    // A column wider than its ceiling does not push the panel apart.
    assert_eq!(
        qd::looks_panel_min_width(0, 900),
        qd::LOOKS_COLUMN_MAX_WIDTH + qd::TOOLS_MARGINS.0 + qd::TOOLS_MARGINS.2
    );
    // The title counts on a par with the column, but the column's ceiling does not
    // apply to it.
    assert_eq!(qd::looks_panel_min_width(300, 100), 316);

    // The window's standard minimum: the original's splitter gives the panel 160, and the labels
    // do not fit in it — the content-based floor must be larger.
    assert!(qd::looks_panel_min_width(142, 216) > qd::split_widths(1100, 750).1);
}

/// ⭐ «Tools» live in the MIDDLE panel, to the right of «Distro
/// looks» (the owner's decision of 28.10.2026), and on a narrow panel —
/// below it in the same column. There is no longer a strip at the bottom of the card.
#[test]
fn tools_sit_right_of_looks_or_below_when_narrow() {
    // A 1920 window: the middle panel is ~629 points — both blocks side by side.
    let wide = qd::split_widths(1920, 1000).1 - qd::TOOLS_MARGINS.0 - qd::TOOLS_MARGINS.2;
    assert!(qd::looks_tools_side_by_side(wide, 200, 216), "панель {wide}");
    let half = qd::looks_tools_block_width(wide, true);
    assert_eq!(half * 2 + qd::LOOKS_TOOLS_GAP, wide - (wide - qd::LOOKS_TOOLS_GAP) % 2);
    // A 1280 window: the panel is not wide enough for two columns — «Tools» below
    // «Distro looks», each block at full width.
    let narrow = qd::split_widths(1280, 700).1 - qd::TOOLS_MARGINS.0 - qd::TOOLS_MARGINS.2;
    assert!(!qd::looks_tools_side_by_side(narrow, 200, 216), "панель {narrow}");
    assert_eq!(qd::looks_tools_block_width(narrow, false), narrow);
    // Exactly on the boundary — side by side; one point narrower — in a column.
    let edge = 2 * 216 + qd::LOOKS_TOOLS_GAP;
    assert!(qd::looks_tools_side_by_side(edge, 150, 216));
    assert!(!qd::looks_tools_side_by_side(edge - 1, 150, 216));
    // A column wider than the ceiling does not "demand" the panel endlessly.
    assert!(qd::looks_tools_side_by_side(1000, 5000, 5000));
    assert_eq!(qd::looks_tools_block_width(-5, true), 0);

    // Layout: the «Tools» block inside the middle panel, the core decides,
    // the card at full height, no strip under it.
    const QML: &str = include_str!("../../rexon-gui/qml/DesktopsPage.qml");
    assert!(!QML.contains("toolsBar"), "полоса «Инструменты» снова под карточкой");
    assert!(QML.contains("objectName: \"toolsBlock\""), "нет блока «Инструменты»");
    let middle = QML.find("id: middleBody").expect("нет тела средней панели");
    let tools = QML.find("objectName: \"toolsBlock\"").unwrap();
    let card = QML.find("id: cardFlick").expect("нет карточки");
    assert!(middle < tools && tools < card, "«Инструменты» не в средней панели");
    assert!(QML.contains("page.looksToolsSideBySide("), "рядом/столбиком решает не ядро");
    assert!(QML.contains("page.toolsNotice"), "пропала строка причины под кнопками");
}

/// The card's left spacer — exactly as in the original's wrapper.
///
/// ⚠️ THE NUMBERS ARE NOT DERIVED BUT MEASURED. Measurement of 05.09: the widget tab was built
/// at eleven window widths (height 6000, skin steam_dark), and after
/// showing the geometry of both elements of the right panel wrapper was read —
/// the spacer and the card. While QML had no spacer, the card's contents
/// started 10 points to the left of the original.
#[test]
fn card_left_spacer_follows_wrapper_stretch() {
    assert_eq!(qd::CARD_WRAP_STRETCH, 100);
    assert_eq!(qd::CARD_MIN_WIDTH, 924);

    // Measured on the original: (viewport width, spacer).
    let measured = [
        (537, 0),
        (577, 0),
        (677, 0),
        (769, 0),
        (852, 0),
        (934, 9),
        (1017, 10),
        (1099, 11),
        (1181, 12),
        (1264, 13),
        (1331, 13),
    ];
    for (viewport, spacer) in measured {
        assert_eq!(
            qd::card_left_spacer(viewport),
            spacer,
            "вьюпорт {viewport}"
        );
        // The card with the spacer cover the viewport entirely, without a gap.
        assert!(viewport - qd::card_left_spacer(viewport) > 0);
    }

    // Empty and negative widths give no spacer.
    assert_eq!(qd::card_left_spacer(0), 0);
    assert_eq!(qd::card_left_spacer(-100), 0);
}


/// The splitter handle is painted with a share of neutral grey from the core, rather than
/// staying light grey from the QtQuick Basic style.
#[test]
fn split_handle_is_painted_from_the_core() {
    use rexon_core::ui_base::{blend_css_over, css_color_to_qml};
    // Measurement of 05.09 on live Qt: (73,83,96) on the page background (27,40,56).
    assert_eq!(
        blend_css_over("#1b2838", qd::split_handle_qss(true)),
        "#495360"
    );
    assert_eq!((0x49, 0x53, 0x60), (73, 83, 96));
    // In QML the colour goes as a ready `#aarrggbb` record, not as a QSS string:
    // the layout reads `rgba(...)` as an invalid colour and paints it black.
    assert_eq!(css_color_to_qml(qd::split_handle_qss(true)), "#33ffffff");
    assert_eq!(css_color_to_qml(qd::split_handle_qss(false)), "#33000000");
}

/// ⭐ Guard of the language of the desktop operation window: questions, refusals and results
/// of installing, checking, switching and removing — in plain Russian for the buyer.
/// The former texts said «DE», «logout/reboot», «через pkexec», «бэкап»;
/// the next such line will go red here, not in a buyer's review.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn operation_texts_have_no_jargon() {
    let _lang = i18n_pin::ru();
    let mut texts: Vec<String> = Vec::new();
    for (installed, available, current) in [(false, true, false), (true, true, false), (true, true, true)] {
        for b in qd::card_buttons(installed, available, current) {
            texts.push(b.text);
            texts.push(b.tooltip);
        }
    }
    for meta in DE_META {
        let name = meta.display_name;
        let i = qd::install_texts(name);
        texts.extend([i.confirm_html, i.done_text, i.done_text_restart, i.operation_title]);
        let r = qd::repair_texts(name);
        texts.extend([r.confirm_html, r.done_text, r.done_text_restart, r.operation_title]);
        let c = qc::install_texts(name);
        texts.extend([c.confirm_html, c.done_text]);
        let s = qc::switch_texts(name);
        texts.extend([s.confirm_text, s.operation_title]);
        for current in ["kde", meta.id] {
            for others in [0_usize, 2] {
                let p = qd::remove_plan(meta.id, name, current, others);
                texts.push(p.title);
                texts.push(p.text);
            }
        }
    }
    for text in &texts {
        for jargon in ["pkexec", "polkit", "install_de", "хелпер", "logout", "reboot", "бэкап"] {
            assert!(!text.contains(jargon), "в тексте для покупателя «{jargon}»: {text}");
        }
        // «DE» only as a separate word: «KDE Plasma» does not count.
        assert!(
            !text.split(|c: char| !c.is_alphanumeric()).any(|w| w == "DE"),
            "в тексте для покупателя «DE» вместо «рабочий стол»: {text}"
        );
    }
}

/// ⭐ The tone of the desktop operation window's buttons is the same on both pages.
///
/// The window (`DeOperationDialog.qml`) is shared by «Desktops» and «Desktop
/// comparison», but it takes the colours of «Yes» from the PAGE palette. «Comparison» did not
/// provide these keys, and «Yes» there came out as an ordinary skin button rather than
/// a blue or red one, as on «Desktops» for the same operation.
/// The guard reads the sources: both pages put the tones with one helper from
/// the core's table, they have no copies of the keys of their own, the window asks for exactly these keys.
#[test]
fn operation_dialog_tones_are_shared_by_both_pages() {
    const DIALOG_QML: &str = include_str!("../../rexon-gui/qml/DeOperationDialog.qml");
    const DESKTOPS_RS: &str = include_str!("../../rexon-gui/src/desktops_page.rs");
    const COMPARE_RS: &str = include_str!("../../rexon-gui/src/compare_page.rs");
    const DESKTOPS_QML: &str = include_str!("../../rexon-gui/qml/DesktopsPage.qml");
    const COMPARE_QML: &str = include_str!("../../rexon-gui/qml/ComparePage.qml");
    // The third owner of the window is «Settings» (installing what is missing).
    const SETTINGS_RS: &str = include_str!("../../rexon-gui/src/settings_page.rs");
    const SETTINGS_QML: &str = include_str!("../../rexon-gui/qml/SettingsPage.qml");
    // The fourth is the main window (restoring the look at startup).
    const MAIN_RS: &str = include_str!("../../rexon-gui/src/main_window.rs");
    const MAIN_QML: &str = include_str!("../../rexon-gui/qml/MainWindow.qml");

    // Colours: a safe operation — the card's blue button, an irreversible one —
    // red, the same as the «Install» and «Remove» buttons.
    let tones: std::collections::BTreeMap<_, _> = qd::OPERATION_DIALOG_TONES.into_iter().collect();
    assert_eq!(tones.len(), 2);
    let primary = tones["primary_tone"];
    let danger = tones["destructive_tone"];
    assert_eq!(primary, qd::PRIMARY_TONE);
    assert_eq!(danger, qd::DESTRUCTIVE_TONE);
    assert!(primary.background.starts_with("rgba(59,130,246,"), "{primary:?}");
    assert!(danger.background.starts_with("rgba(220,80,80,"), "{danger:?}");
    assert_ne!(primary.background, danger.background);
    for t in [primary, danger] {
        assert_eq!(t.radius, qd::CARD_BUTTON_RADIUS);
        assert_eq!(t.color, qd::ACCENT_BUTTON_TEXT_COLOR);
        assert!(t.bold);
    }

    // The window asks for exactly the table's keys and chooses «Yes» by the danger of the operation.
    for key in tones.keys() {
        assert!(
            DIALOG_QML.contains(&format!("root.pal.{key}")),
            "окно операции не читает `{key}` из палитры страницы"
        );
    }
    assert!(DIALOG_QML.contains("dangerTone: root.pal.destructive_tone"));
    assert!(DIALOG_QML.contains("root.pal.primary_tone"));
    assert!(DIALOG_QML.contains("(root.op && root.op.danger) ? root.dangerTone : root.primaryTone"));

    // Both pages put the tones into the palette with one helper and do not create
    // copies of the keys of their own.
    fn palette_body(src: &str) -> &str {
        let start = src.find("fn palette(&self)").expect("у страницы нет fn palette");
        let rest = &src[start..];
        let end = rest.find("\n    }\n").expect("конец fn palette не найден");
        &rest[..end]
    }
    for (page, src) in [
        ("desktops_page.rs", DESKTOPS_RS),
        ("compare_page.rs", COMPARE_RS),
        ("settings_page.rs", SETTINGS_RS),
        ("main_window.rs", MAIN_RS),
    ] {
        assert!(
            palette_body(src).contains("insert_operation_tones(&mut out)"),
            "{page}: палитра не отдаёт тоны окна операции общим помощником"
        );
        for key in tones.keys() {
            assert!(
                !src.contains(&format!("QString::from(\"{key}\")")),
                "{page}: своя копия ключа `{key}` мимо таблицы ядра"
            );
        }
    }
    assert!(DESKTOPS_RS.contains("for (key, t) in qd::OPERATION_DIALOG_TONES"));
    for (page, src) in [
        ("DesktopsPage.qml", DESKTOPS_QML),
        ("ComparePage.qml", COMPARE_QML),
        ("SettingsPage.qml", SETTINGS_QML),
        ("MainWindow.qml", MAIN_QML),
    ] {
        for key in tones.keys() {
            assert!(
                !src.contains(&format!("{key}:")),
                "{page}: тон `{key}` задан в разметке мимо палитры"
            );
        }
        assert!(src.contains("DeOperationDialog {"), "{page}: окно операции не общее");
        assert!(src.contains("pal: root.pal"), "{page}: окну не отдана палитра страницы");
    }
}

/// ⭐ Guard of «DE» in labels for the buyer outside the operation window:
/// the comparison name (window title, header, subsection), the COSMIC hint,
/// desktop descriptions, notes, warnings and compatibility refusals of
/// distributions, the check log. The product says «рабочий стол» with the same
/// words as the operation window (`operation_texts_have_no_jargon`); a new
/// «DE» here goes red before the buyer sees it.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn visible_texts_say_desktop_not_de() {
    let _lang = i18n_pin::ru();
    use rexon_core::compliance;
    use rexon_core::distro_compat::{InstallReason, DISTRO_INFO};
    use rexon_core::{qt_app, qt_main};

    let mut texts: Vec<String> = vec![
        qc::window_title(),
        qc::header_title(),
        qd::cosmic_hint(),
        qt_main::subsection_label("compare"),
        compliance::msg_unknown_de(),
    ];
    texts.extend(compliance::de_banner_lines("kde"));
    for (_, id) in qt_app::ENTRY_IDENTITIES {
        texts.extend(id.display_name.map(str::to_string));
    }
    for (_, info) in DISTRO_INFO {
        texts.push(info.notes.to_string());
        texts.extend(info.warnings.iter().map(|w| w.to_string()));
    }
    for reason in [
        InstallReason::AstraNoRepos { de_id: "kde".into() },
        InstallReason::OsnovaClosed,
        InstallReason::AltUnsupportedDe { de_id: "cosmic".into() },
    ] {
        texts.push(reason.short_message());
    }
    for meta in DE_META {
        texts.push(meta.description.to_string());
    }
    assert!(texts.len() > 40, "разбор подписей опустел: {}", texts.len());
    for text in &texts {
        assert!(
            !text.split(|c: char| !c.is_alphanumeric()).any(|w| w == "DE"),
            "в тексте для покупателя «DE» вместо «рабочий стол»: {text}"
        );
    }
}

/// SteamOS (Steam Deck and Steam Machine): exactly those buttons become inactive
/// that touch system packages — install, install missing, remove and
/// «Check my desktop». Switching, comparison and panel backups stay:
/// they do not touch packages and work in the home folder.
#[test]
fn steamos_disables_only_package_actions() {
    let mut kinds: Vec<&str> = Vec::new();
    for (installed, current) in [(false, false), (true, false), (true, true)] {
        kinds.extend(qd::card_buttons(installed, true, current).iter().map(|b| b.kind));
    }
    kinds.extend(qd::tool_buttons("kde", true).iter().map(|b| b.kind));
    kinds.sort_unstable();
    kinds.dedup();
    let blocked: Vec<&str> = kinds.iter().copied().filter(|k| qd::is_package_action(k)).collect();
    let kept: Vec<&str> = kinds.iter().copied().filter(|k| !qd::is_package_action(k)).collect();
    assert_eq!(blocked, ["install", "remove", "repair", "repair_current"]);
    assert_eq!(kept, ["compare", "panel_backup", "panel_restore", "switch"]);
    for kind in &kept {
        assert_eq!(qd::steamos_button_block(kind, "gnome", false, true), None, "{kind}");
    }
}

/// SteamOS: tested desktops (Xfce, LXQt, Hyprland) are installed and removed
/// as systemd-sysext images — their buttons are live; Plasma (SteamOS's own desktop) and
/// untested ones are inactive with a reason. Rebuilding the desktop that is in use
/// right now is not offered. Not on SteamOS — nothing is disabled.
#[test]
fn steamos_buttons_follow_sysext_support() {
    use rexon_core::distro_compat::{steamos_desktop_card_block, SteamOsBlock};
    let desktops = SteamOsBlock::Desktops.reason();
    for de in ["xfce", "lxqt", "hyprland"] {
        for kind in ["install", "repair", "remove"] {
            assert_eq!(qd::steamos_button_block(kind, de, false, true), None, "{de} {kind}");
        }
        assert_eq!(qd::steamos_button_block("repair", de, true, true), Some(desktops.clone()), "{de}");
    }
    assert_eq!(qd::steamos_button_block("repair", "kde", true, true), Some(desktops.clone()));
    assert_eq!(qd::steamos_button_block("repair_current", "xfce", true, true), Some(desktops.clone()));
    for de in ["gnome", "cosmic", "mate"] {
        let why = qd::steamos_button_block("install", de, false, true).expect(de);
        assert_eq!(Some(why.clone()), steamos_desktop_card_block(de, true));
        assert_ne!(why, desktops, "{de}: причина должна называть проверенные столы");
    }
    for de in ["xfce", "kde", "gnome"] {
        for kind in ["install", "repair", "remove", "repair_current"] {
            assert_eq!(qd::steamos_button_block(kind, de, true, false), None, "{de} {kind}");
        }
    }
}

/// SteamOS: the install question for an image desktop is the `steamos_sysext` text (not
/// the package one «choose on the login screen»), removal has its own question.
/// Other desktops and non-SteamOS — the ordinary texts.
#[test]
fn steamos_sysext_desktops_get_their_own_texts() {
    use rexon_core::steamos_sysext as sx;
    let base = qd::install_texts("Xfce");
    let t = qd::steamos_op_texts(qd::install_texts("Xfce"), "xfce", Some("3.9.5"));
    assert_eq!(t.confirm_html, sx::install_confirm_text("Xfce", "3.9.5"));
    // The result is the executor's line in the log, there is no note under the log.
    assert!(t.done_text.is_empty() && t.done_text_restart.is_empty());
    assert_eq!(t.operation_title, base.operation_title);
    let r = qd::steamos_op_texts(qd::repair_texts("Xfce"), "xfce", Some("3.9.5"));
    assert_eq!(r.confirm_html, sx::install_confirm_text("Xfce", "3.9.5"));
    assert_eq!(r.operation_title, qd::repair_texts("Xfce").operation_title);
    for (de, ver) in [("xfce", None), ("gnome", Some("3.9.5")), ("kde", Some("3.9.5"))] {
        let t = qd::steamos_op_texts(qd::install_texts("X"), de, ver);
        assert_eq!(t.confirm_html, qd::install_texts("X").confirm_html, "{de}");
    }
    assert_eq!(qd::steamos_remove_text("lxqt", true), Some(sx::remove_confirm_text("LXQt")));
    assert_eq!(qd::steamos_remove_text("lxqt", false), None);
    assert_eq!(qd::steamos_remove_text("kde", true), None);
}

// ── English interface ───────────────────────────────────────────────────

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// All the tab's labels assembled by the core, in English — without a single
/// Russian letter: a forgotten string shows up here, not to the buyer.
///
/// Desktop descriptions (`DeMeta::description_text`) and switch texts
/// (`qt_compare::switch_texts`) are translated by the «Comparison» pass — they are guarded by
/// its check, they are not collected here.
#[test]
fn english_desktops_tab_has_no_russian() {
    let _lang = i18n_pin::en();
    let mut all: Vec<String> = vec![
        qd::window_title(),
        qd::list_header(),
        qd::section_classic(),
        qd::section_tiling(),
        qd::section_effects(),
        qd::cosmic_hint(),
        qd::tip_installed(),
        qd::tip_unavailable(),
        qd::tools_header(),
        qd::compare_back_text(),
        qd::placeholder_text(),
        qd::status_installed(),
        qd::status_not_installed(),
        qd::status_unavailable(),
        qd::unavailable_notice(),
        qd::current_badge_text(),
        qd::current_badge_tooltip(),
        qd::version_checking(),
        qd::version_line("6.1"),
        qd::no_screenshots_text(),
        qd::carousel_zoom_tooltip(),
        qd::carousel_prev_tooltip(),
        qd::carousel_next_tooltip(),
        qd::carousel_zoom_button_tooltip(),
        qd::zoom_prev_tooltip(),
        qd::zoom_next_tooltip(),
        qd::zoom_close_tooltip(),
        qd::zoom_title(),
        qd::zoom_hint(0, 0),
        qd::zoom_hint(1, 3),
        qd::packages_text(1, 0),
        qd::packages_text(12, 3),
        qd::repair_current_unknown_title(),
        qd::repair_current_unknown_text(),
        qd::panel_restore_confirm_title(),
        qd::remove_operation_title("XFCE"),
    ];
    for row in qd::list_rows(PkgFamily::Arch, &|id| id == "cosmic") {
        all.extend([row.text, row.tooltip]);
    }
    for b in qd::tool_buttons("kde", true) {
        all.extend([b.text, b.tooltip]);
    }
    for (installed, current) in [(false, false), (true, false), (true, true)] {
        for b in qd::card_buttons(installed, true, current) {
            all.extend([b.text, b.tooltip]);
        }
    }
    for cell in qd::stats("kde") {
        all.extend([cell.value, cell.label]);
    }
    for t in [qd::install_texts("KDE Plasma"), qd::repair_texts("XFCE")] {
        all.extend([
            t.confirm_title,
            t.confirm_html,
            t.operation_title,
            t.done_title,
            t.done_text,
            t.done_text_restart,
        ]);
    }
    for others in [0_usize, 2] {
        for current in ["kde", "xfce"] {
            let p = qd::remove_plan("xfce", "XFCE", current, others);
            all.extend([p.title, p.text]);
        }
    }
    for ok in [true, false] {
        let p = qd::panel_backup_result(ok, "x");
        all.extend([p.title, p.text]);
        let p = qd::panel_restore_result(ok, "x");
        all.extend([p.title, p.text]);
    }
    assert!(all.len() > 80, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
        // The same jargon guard as for the Russian operation window.
        for jargon in ["pkexec", "polkit", "install_de"] {
            assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
        }
        assert!(
            !text.split(|c: char| !c.is_alphanumeric()).any(|w| w == "DE"),
            "«DE» вместо «desktop»: {text}"
        );
    }
}

/// English phrases assembled from parts read as phrases.
#[test]
fn english_desktops_texts_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(qd::packages_text(1, 0), "📦 1 package to install  (all from official repositories)");
    assert_eq!(qd::packages_text(12, 3), "📦 15 packages to install  (12 main + 3 AUR)");
    assert_eq!(
        qd::zoom_hint(1, 3),
        "× in the corner, click the picture or Esc to close · ←/→ to browse  ·  2/3"
    );
    assert_eq!(qd::version_line("6.1.2-1"), "Available in your distribution: 6.1.2-1");
    assert_eq!(qd::item_tooltip("cosmic", true, false),
        "Installed · Hybrid: a desktop with optional tiling · Not available on this distribution");
    let t = qd::install_texts("KDE Plasma");
    assert_eq!(t.confirm_title, "Install?");
    assert_eq!(
        t.confirm_html,
        "Install the <b>KDE Plasma</b> desktop?<br><br>Rexon will ask for the administrator password."
    );
    assert_eq!(t.operation_title, "Installing KDE Plasma");
    assert_eq!(t.done_text, "<b>KDE Plasma</b> is installed.\nLog out and choose KDE Plasma on the login screen.");
    assert_eq!(
        qd::repair_current_unknown_text(),
        "Couldn't detect your current desktop — choose it in the list on the left and click \
“Check / Repair” on its card."
    );
    let p = qd::remove_plan("xfce", "XFCE", "kde", 2);
    assert_eq!(
        p.text,
        "Remove the <b>XFCE</b> desktop?\n\nCurrent desktop: <b>KDE Plasma</b> — it won't be \
affected.\nAll of this desktop's packages will be removed. Rexon will ask for the administrator \
password."
    );
    assert_eq!(qd::stats("kde")[0].value, format!("{} MB", DE_META.iter().find(|m| m.id == "kde").unwrap().ram_mb));
}

// ── The «Distro looks» block ────────────────────────────────────────────

mod looks {
    use super::*;
    use rexon_core::distro_looks::{self as dl, Probes, Target};
    use std::collections::BTreeMap;

    /// A desktop without a single write to disk: the plan is built from probes, the profile is not
    /// needed at all (the directories point to nonexistent places).
    fn target(de: &str) -> Target {
        let h = "/nonexistent/rexon-looks-home".to_string();
        Target {
            de: de.into(),
            session: dl::session_for(de, BTreeMap::new()),
            family: PkgFamily::Arch,
            config: format!("{h}/.config"),
            data: format!("{h}/.local/share"),
            cache: format!("{h}/.cache"),
            home: h,
            lang: "en".into(),
            wayland: false,
            cursor_size: None,
            aur_helper: None,
        }
    }

    /// The «Like SteamOS» offer on Plasma. `aur_missing` — the game mode packages
    /// from AUR are absent (they are not in the repositories either); otherwise everything is installed.
    fn steamos_offer(aur_missing: bool, applied: &dyn Fn(&str) -> bool) -> dl::Offer {
        let missing = move |n: &str| aur_missing && n.contains("gamescope-session");
        let owner = |n: &str| (!missing(n)).then(|| n.to_string());
        let in_repos = |n: &str| !missing(n);
        let no_ext = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &in_repos, extension_installed: &no_ext };
        dl::offer(dl::builtin("steamos").unwrap().unwrap(), &target("kde"), &probes, applied)
    }

    /// Default options.
    fn def() -> rexon_core::rxn_manifest::PropValues {
        Default::default()
    }

    fn vgui() -> rexon_core::rxn_manifest::PropValues {
        [("variant".to_string(), serde_json::Value::String("vgui".into()))].into_iter().collect()
    }

    /// The SteamOS style is a switch in the question window: Vapor by default and
    /// VGUI2, labels from the manifest. VGUI2 pulls in the classic KDE cursors
    /// (oxygen-cursors) — they are absent, and switching immediately changes both the text and
    /// the plan, without new system probes.
    #[test]
    fn steamos_style_is_a_switch() {
        let _lang = i18n_pin::en();
        let owner = |n: &str| (n != "oxygen-cursors").then(|| n.to_string());
        let yes = |_: &str| true;
        let no_ext = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &yes, extension_installed: &no_ext };
        let offer = dl::offer(dl::builtin("steamos").unwrap().unwrap(), &target("kde"), &probes, &|_| false);
        assert_eq!(offer.variants.len(), 2);
        let choices = match qd::look_confirm(&offer, &def(), false) {
            qd::LookConfirm::Ask { choices, text, .. } => {
                assert!(!text.contains("oxygen-cursors"), "{text}");
                choices
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].name, "variant");
        assert_eq!(choices[0].label, "Style");
        assert_eq!(choices[0].current, "vapor");
        let labels: Vec<&str> = choices[0].options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Vapor (Steam Deck)", "VGUI2 (classic Steam)"]);
        match qd::look_confirm(&offer, &vgui(), false) {
            qd::LookConfirm::Ask { choices, text, .. } => {
                assert_eq!(choices[0].current, "vgui");
                assert!(text.contains("oxygen-cursors"), "{text}");
            }
            other => panic!("{other:?}"),
        }
        let plan = qd::look_chosen_plan(&offer, &vgui(), false).unwrap();
        assert_eq!(plan.values.get("variant").and_then(|v| v.as_str()), Some("vgui"));
        assert!(plan.install.iter().any(|d| d.resolved.install_names().contains(&"oxygen-cursors".to_string())));
        assert!(qd::look_chosen_plan(&offer, &def(), false).unwrap().install.is_empty());
        // The style choice and the game mode are independent.
        assert!(qd::look_chosen_plan(&offer, &vgui(), true).unwrap().items.iter().any(|i| i.kind == "game-session"));
        // Garuda has no lists — and no switches.
        let garuda = dl::offer(dl::builtin("garuda-dr460nized").unwrap().unwrap(), &target("kde"), &probes, &|_| false);
        assert_eq!(garuda.variants.len(), 1);
        if let qd::LookConfirm::Ask { choices, .. } = qd::look_confirm(&garuda, &def(), false) {
            assert!(choices.is_empty());
        }
    }

    fn ask(c: qd::LookConfirm) -> (String, String, Option<qd::LookOption>) {
        match c {
            qd::LookConfirm::Ask { title, text, option, .. } => (title, text, option),
            qd::LookConfirm::Blocked { title, text } => panic!("отказ: {title} — {text}"),
        }
    }

    /// A tile for each built-in image, the name comes from the manifest; the
    /// applied one has a mark and «Restore», the others have «Apply».
    #[test]
    fn tiles_follow_builtin_looks() {
        let _lang = i18n_pin::en();
        let tiles = qd::look_tiles("en", &|id| id == "garuda-dr460nized");
        let ids: Vec<&str> = tiles.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, dl::builtin_ids());
        let steam = tiles.iter().find(|t| t.id == "steamos").unwrap();
        assert_eq!(steam.name, "Like SteamOS");
        assert!(!steam.applied && steam.badge.is_empty() && !steam.description.is_empty());
        assert_eq!(steam.buttons.iter().map(|b| b.kind).collect::<Vec<_>>(), ["look_apply"]);
        let garuda = tiles.iter().find(|t| t.id == "garuda-dr460nized").unwrap();
        assert!(garuda.applied);
        assert_eq!(garuda.badge, "✓ Applied");
        assert_eq!(garuda.buttons.iter().map(|b| b.kind).collect::<Vec<_>>(), ["look_revert"]);
        // Russian names also come from the manifest, not from the translation catalog.
        assert_eq!(qd::look_tiles("ru", &|_| false)[0].name, "Как в SteamOS");
    }

    /// The SteamOS game mode is opt-in only: without the checkbox it is neither in
    /// "what will change" nor in "will not be applied", and the plan without it goes
    /// to applying; with the checkbox — the plan with it. Under the checkbox there is a warning from
    /// the manifest.
    #[test]
    fn steamos_game_mode_is_opt_in() {
        let _lang = i18n_pin::en();
        let offer = steamos_offer(false, &|_| false);
        let (title, text, option) = ask(qd::look_confirm(&offer, &def(), false));
        assert_eq!(title, "Apply “Like SteamOS”?");
        let option = option.expect("галочка игрового режима");
        assert_eq!(option.text, "Also turn on game mode");
        assert!(option.hint.contains("Steam Big Picture"), "{}", option.hint);
        assert!(!text.contains("game mode"), "{text}");
        assert!(text.contains("Put Back as It Was"), "{text}");
        let off = qd::look_chosen_plan(&offer, &def(), false).unwrap();
        assert!(!off.items.iter().any(|i| i.kind == "game-session"));
        let (_, text_on, _) = ask(qd::look_confirm(&offer, &def(), true));
        assert!(text_on.contains("game mode"), "{text_on}");
        let on = qd::look_chosen_plan(&offer, &def(), true).unwrap();
        assert!(on.items.iter().any(|i| i.kind == "game-session"));
    }

    /// The game mode needs packages from AUR, and they are absent: with the checkbox the window
    /// honestly says "will not be applied" and names the packages — Rexon does not build them
    /// itself; the rest of the look is applied.
    #[test]
    fn game_mode_from_aur_is_named_not_installed() {
        let _lang = i18n_pin::en();
        let offer = steamos_offer(true, &|_| false);
        let (_, text, option) = ask(qd::look_confirm(&offer, &def(), true));
        assert!(option.is_some());
        assert!(text.contains("Won't be applied: game mode"), "{text}");
        assert!(text.contains("aur/gamescope-session-git"), "{text}");
        let plan = qd::look_chosen_plan(&offer, &def(), true).unwrap();
        assert!(!plan.items.iter().any(|i| i.kind == "game-session"));
        assert!(plan.install.iter().all(|d| !d.resolved.install_names().iter().any(|n| n.starts_with("aur/"))));
        // Without the checkbox not a word about AUR: the person did not ask for it.
        let (_, plain, _) = ask(qd::look_confirm(&offer, &def(), false));
        assert!(!plain.contains("AUR"), "{plain}");
    }

    /// Something required does not install (libplasma replaced by a fork) — a refusal window,
    /// nothing to apply.
    #[test]
    fn refused_look_is_a_blocked_window() {
        let _lang = i18n_pin::en();
        let owner = |n: &str| Some(if n == "libplasma" { "aeroshell-libplasma".to_string() } else { n.to_string() });
        let yes = |_: &str| true;
        let no_ext = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &yes, extension_installed: &no_ext };
        let offer = dl::offer(dl::builtin("steamos").unwrap().unwrap(), &target("kde"), &probes, &|_| false);
        match qd::look_confirm(&offer, &def(), false) {
            qd::LookConfirm::Blocked { title, text } => {
                assert_eq!(title, "“Like SteamOS” can't be applied");
                assert!(text.contains("libplasma"), "{text}");
            }
            other => panic!("ждали отказ: {other:?}"),
        }
        assert!(qd::look_chosen_plan(&offer, &def(), false).is_none());
    }

    /// Another image is already applied — it will be restored before this, and the window
    /// says so first of all.
    #[test]
    fn applied_other_look_is_put_back_first() {
        let _lang = i18n_pin::en();
        let offer = steamos_offer(false, &|id| id == "garuda-dr460nized" || id == "steamos");
        assert_eq!(offer.replaces.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["garuda-dr460nized"]);
        let (_, text, _) = ask(qd::look_confirm(&offer, &def(), false));
        assert!(text.contains("First, “Like Garuda Linux (Dr460nized)” will be put back as it was."), "{text}");
    }

    /// The block's Russian labels come from `po/ru.po`.
    #[test]
    fn looks_texts_in_russian() {
        let _lang = i18n_pin::ru();
        assert_eq!(qd::looks_header(), "🎨 Как в дистрибутиве");
        let tiles = qd::look_tiles("ru", &|id| id == "steamos");
        assert_eq!(tiles[0].badge, "✓ Применён");
        assert_eq!(tiles[0].buttons[0].text, "↩️ Вернуть как было");
        assert_eq!(tiles[1].buttons[0].text, "Применить");
        assert_eq!(qd::look_revert_confirm("Как в SteamOS").2, "Вернуть");
    }

    /// All of the block's labels in English — without a single Russian letter.
    #[test]
    fn looks_texts_in_english_have_no_russian() {
        let _lang = i18n_pin::en();
        let mut all = vec![
            qd::looks_header(),
            qd::looks_hint(),
            qd::look_apply_title("X"),
            qd::look_checking(),
            qd::look_unknown_desktop().0,
            qd::look_unknown_desktop().1,
            qd::look_revert_title("X"),
        ];
        let (a, b, c) = qd::look_revert_confirm("X");
        all.extend([a, b, c]);
        for t in qd::look_tiles("en", &|id| id == "steamos") {
            all.extend([t.name, t.description, t.badge]);
            for b in t.buttons {
                all.extend([b.text, b.tooltip]);
            }
        }
        for aur in [false, true] {
            let offer = steamos_offer(aur, &|_| true);
            for on in [false, true] {
                if let qd::LookConfirm::Ask { title, text, yes_text, no_text, option, .. } = qd::look_confirm(&offer, &def(), on) {
                    all.extend([title, text, yes_text, no_text]);
                    if let Some(o) = option {
                        all.extend([o.text, o.hint]);
                    }
                }
            }
        }
        for text in &all {
            assert!(!cyrillic(text), "русская строка в английском интерфейсе: {text}");
        }
    }
}

// ── X11 or Wayland session at install time ──────────────────────────────

/// There is a choice exactly for desktops with two sessions in Arch; Plasma 6.8,
/// GNOME 51, Budgie 10.10, Pantheon 8, COSMIC and Deepin have one session — Wayland.
/// MATE is installed X11 only (16.11.2026: the former Wayland session from AUR was dropped;
/// the AUR session mechanism is under the core's module tests on a sample).
/// There are three options: X11, Wayland and both; each has a card explanation.
#[test]
fn session_choice_only_where_two_sessions_exist() {
    use system::SessionPick;
    let _lang = i18n_pin::en();
    let two = ["cinnamon-mint", "xfce", "lxqt"];
    for de in system::list_de_ids() {
        let choice = qd::session_choice(de, PkgFamily::Arch, SessionPick::X11, true);
        assert_eq!(choice.is_some(), two.contains(&de), "{de}");
        // Outside Arch the set is not verified — no choice, installed as before.
        assert!(qd::session_choice(de, PkgFamily::Debian, SessionPick::X11, true).is_none(), "{de}");
    }
    for de in two {
        let c = qd::session_choice(de, PkgFamily::Arch, SessionPick::Both, true).unwrap();
        assert_eq!(c.name, qd::SESSION_CHOICE_NAME);
        assert_eq!(c.current, "both");
        let values: Vec<&str> = c.options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, ["x11", "wayland", "both"], "{de}");
        for o in &c.options {
            assert_eq!(SessionPick::from_value(&o.value).map(|k| k.as_str()), Some(o.value.as_str()));
            assert!(o.enabled, "{de}: {o:?}");
            assert!(!o.description.is_empty(), "{de}: {o:?}");
        }
    }
    // Without an AUR helper the choice stays for desktops with Wayland from the repositories.
    let xfce = qd::session_choice("xfce", PkgFamily::Arch, SessionPick::Wayland, false).unwrap();
    assert!(xfce.options.iter().all(|o| o.enabled));
    assert_eq!(xfce.current, "wayland");
}

/// The set per option: «Wayland» and «Both» install the Wayland add-on, «X11» does not.
#[test]
fn session_pick_sets_install_kind() {
    use system::{SessionKind, SessionPick};
    assert_eq!(SessionPick::X11.install_kind(), SessionKind::X11);
    assert_eq!(SessionPick::Wayland.install_kind(), SessionKind::Wayland);
    assert_eq!(SessionPick::Both.install_kind(), SessionKind::Wayland);
    assert_eq!(SessionPick::from_value("nope"), None);
    let arch = PkgFamily::Arch;
    for pick in SessionPick::ALL {
        // MATE has no choice — the window will not even ask; what came in is what
        // stays (the set without the add-on, see `session_packages_by_choice`).
        assert_eq!(qd::session_pick_effective("mate", arch, pick, false), pick);
        assert_eq!(qd::session_pick_effective("xfce", arch, pick, false), pick);
    }
}

/// The recommended option and the labels come from the Wayland maturity flag in
/// the table: mature — Wayland «(recommended)», experimental — the
/// proven X11. As of 03.11.2026 only LXQt's is mature (the project dropped the mark
/// in 2.3.0).
#[test]
fn session_default_follows_wayland_maturity() {
    use system::{SessionPick, WaylandMaturity};
    let _lang = i18n_pin::ru();
    let arch = PkgFamily::Arch;
    for (de, v) in system::SESSION_VARIANTS {
        let want = match v.maturity {
            WaylandMaturity::Mature => SessionPick::Wayland,
            _ => SessionPick::X11,
        };
        assert_eq!(v.default_pick(), want, "{de}");
        assert_eq!(v.default_pick().install_kind(), v.default_session(), "{de}");
        assert_eq!(qd::session_default(de, arch), Some(want), "{de}");
        let c = qd::session_choice(de, arch, want, true).unwrap();
        assert_eq!(c.current, want.as_str(), "{de}");
        let labels: Vec<String> = c.options.iter().map(|o| o.label.clone()).collect();
        let recommended: Vec<&String> = labels.iter().filter(|l| l.contains("(рекомендуется)")).collect();
        assert_eq!(recommended.len(), 1, "{de}: {labels:?}");
        let meta = system::DE_META.iter().find(|m| m.id == *de).expect("стол в DE_META");
        let way = &c.options[1].description;
        assert!(c.options[0].description.contains("старых компьютеров"), "{de}: {:?}", c.options[0]);
        assert!(way.contains("без разрывов"), "{de}: {way}");
        assert!(c.options[2].description.contains("экране входа"), "{de}: {:?}", c.options[2]);
        if v.maturity == WaylandMaturity::Mature {
            assert_eq!(labels[0], "X11 — проверенный", "{de}");
            assert_eq!(labels[1], "Wayland — новее (рекомендуется)", "{de}");
            assert_eq!(meta.session_text(), "X11 + Wayland", "{de}");
            assert!(!way.contains("экспериментальн"), "{de}: {way}");
        } else {
            assert_eq!(labels[0], "X11 — проверенный (рекомендуется)", "{de}");
            assert!(meta.session_text().starts_with("X11 + Wayland (экспериментально"), "{de}");
            assert!(way.contains("экспериментальный"), "{de}: {way}");
        }
        // A third-party window compositor — stated on the Wayland card.
        if !v.wayland_compositor.is_empty() {
            assert!(way.contains(v.wayland_compositor), "{de}: {way}");
        }
    }
    let state = |de: &str| system::session_variants(de, arch).map(|v| v.maturity);
    assert_eq!(state("lxqt"), Some(WaylandMaturity::Mature));
    assert_eq!(state("cinnamon-mint"), Some(WaylandMaturity::Experimental));
    assert_eq!(state("xfce"), Some(WaylandMaturity::Experimental));
    assert_eq!(state("mate"), None);

    drop(_lang);
    let _en = i18n_pin::en();
    let lxqt = qd::session_choice("lxqt", arch, SessionPick::Wayland, true).unwrap();
    assert_eq!(lxqt.options[1].label, "Wayland — newer (recommended)");
    assert_eq!(lxqt.options[0].label, "X11 — proven");
    assert_eq!(lxqt.options[2].label, "Both (choose on the login screen)");
    let way = qd::install_texts_with_session("lxqt", arch, SessionPick::Wayland, true, "").confirm_html;
    assert!(!way.contains("experimental"), "{way}");
}

/// The set by choice: X11 — the main set, Wayland — plus the add-on; the add-on
/// is part of the desktop's packages (privilege session, protection of the current desktop).
#[test]
fn session_packages_by_choice() {
    use system::SessionKind;
    let arch = PkgFamily::Arch;
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("cinnamon-mint", &[], &[]),
        ("xfce", &["labwc", "xorg-xwayland"], &[]),
        ("lxqt", &["lxqt-wayland-session", "labwc", "xorg-xwayland", "swaylock"], &[]),
    ];
    for (de, repo, aur) in cases {
        assert!(system::session_packages(de, arch, SessionKind::X11).is_empty(), "{de}");
        assert!(system::session_aur_packages(de, arch, SessionKind::X11).is_empty(), "{de}");
        assert_eq!(system::session_packages(de, arch, SessionKind::Wayland), *repo, "{de}");
        assert_eq!(system::session_aur_packages(de, arch, SessionKind::Wayland), *aur, "{de}");
        let all = system::desktop_packages(de, arch);
        for p in repo.iter().chain(aur.iter()) {
            assert!(all.iter().any(|a| a == p), "{de}: {p}");
        }
        // The session bundle is removed with the desktop, the compositors are not.
        for p in system::session_removal_packages(de, arch) {
            assert!(repo.contains(&p.as_str()) || aur.contains(&p.as_str()), "{de}: {p}");
            assert!(!["labwc", "wayfire", "xorg-xwayland"].contains(&p.as_str()), "{de}: {p}");
        }
    }
    // MATE is X11 only: neither Wayfire nor `mate-wayland-session` is in its set.
    assert!(system::session_variants("mate", arch).is_none());
    for kind in [SessionKind::X11, SessionKind::Wayland] {
        assert!(system::session_packages("mate", arch, kind).is_empty());
        assert!(system::session_aur_packages("mate", arch, kind).is_empty());
    }
    let mate = system::desktop_packages("mate", arch);
    assert!(!mate.iter().any(|p| p == "wayfire" || p == "mate-wayland-session"), "{mate:?}");
    // …but a `mate-wayland-session` installed by an earlier release still goes
    // away with MATE: it holds MATE's packages as a dependency (16.11.2026).
    assert_eq!(system::session_removal_packages("mate", arch), ["mate-wayland-session"]);
    assert!(system::session_removal_packages("mate", PkgFamily::Debian).is_empty());
    // The package helper's allow-list knows the add-on.
    let allow = rexon_core::system_plan::pkg_helper_install_allowlist(arch);
    for p in ["labwc", "xorg-xwayland", "lxqt-wayland-session", "wayfire"] {
        assert!(allow.iter().any(|a| a == p), "{p}");
    }
}

/// "Can one already log into Wayland": the session file and the compositor.
#[test]
fn wayland_session_ready_needs_file_and_compositor() {
    let arch = PkgFamily::Arch;
    let file = |f: &str| f == "/usr/share/wayland-sessions/xfce-wayland.desktop";
    assert_eq!(system::wayland_session_ready("xfce", arch, &file, &|_| false), Some(false));
    assert_eq!(system::wayland_session_ready("xfce", arch, &file, &|b| b == "labwc"), Some(true));
    assert_eq!(system::wayland_session_ready("xfce", arch, &|_| false, &|_| true), Some(false));
    // LXQt takes any compositor it knows.
    let lxqt = |f: &str| f.ends_with("lxqt-wayland.desktop");
    assert_eq!(system::wayland_session_ready("lxqt", arch, &lxqt, &|b| b == "kwin_wayland"), Some(true));
    assert_eq!(system::wayland_session_ready("kde", arch, &|_| true, &|_| true), None);
}

/// Window texts: the result names the session the way the login screen writes it (for
/// «Both» — both names); desktops without an AUR session have no «AUR» mark.
/// The AUR warning is a core module test on a sample.
#[test]
fn session_texts_follow_choice() {
    use system::{SessionKind, SessionPick};
    let _lang = i18n_pin::en();
    let arch = PkgFamily::Arch;
    let x11 = qd::install_texts_with_session("xfce", arch, SessionPick::X11, true, "");
    let way = qd::install_texts_with_session("xfce", arch, SessionPick::Wayland, true, "");
    let both = qd::install_texts_with_session("xfce", arch, SessionPick::Both, true, "");
    assert!(x11.done_text.contains("“Xfce Session”"), "{}", x11.done_text);
    assert!(way.done_text.contains("“Xfce Session (Wayland)”"), "{}", way.done_text);
    assert!(way.done_text_restart.contains("“Xfce Session (Wayland)”"));
    assert!(
        both.done_text.contains("“Xfce Session”") && both.done_text.contains("“Xfce Session (Wayland)”"),
        "{}",
        both.done_text
    );
    // Explanations are on the cards: for Xfce the question stays as before.
    let plain = qd::install_texts("XFCE").confirm_html;
    assert_eq!(way.confirm_html, plain);
    assert_eq!(x11.confirm_html, plain);
    let xfce_choice = qd::session_choice("xfce", arch, SessionPick::X11, true).unwrap();
    assert!(!xfce_choice.options[1].label.contains("AUR"));

    assert!(!qd::install_texts_with_session("xfce", arch, SessionPick::Wayland, true, "RISK-LINE")
        .confirm_html
        .contains("RISK-LINE"));
    // MATE is X11 only: a question without a session choice and without AUR.
    let mate = qd::install_texts_with_session("mate", arch, SessionPick::Wayland, false, "RISK-LINE");
    assert_eq!(mate.confirm_html, qd::install_texts(qd::display_name("mate")).confirm_html);

    let add = qd::session_add_texts("lxqt", arch, SessionKind::Wayland, true, "");
    assert!(add.done_text.contains("“LXQt (Wayland)”"), "{}", add.done_text);
    let add_x11 = qd::session_add_texts("lxqt", arch, SessionKind::X11, true, "");
    assert!(add_x11.done_text.contains("“LXQt Desktop”"), "{}", add_x11.done_text);
    assert!(add_x11.confirm_html.contains("<b>X11</b>"), "{}", add_x11.confirm_html);
    assert_ne!(qd::session_offer_button(SessionKind::Wayland), qd::session_offer_button(SessionKind::X11));
    assert!(qd::session_offer_button(SessionKind::X11).contains("X11"));
    assert!(qd::session_offer_hint("lxqt", SessionKind::Wayland).contains("LXQt"));
    assert!(qd::session_offer_hint("lxqt", SessionKind::X11).contains("X11"));
}

/// The installed desktop's card line and the missing session button — by
/// the session files and the programs in the system.
#[test]
fn installed_sessions_line_and_missing_offer() {
    use system::SessionKind;
    let _lang = i18n_pin::ru();
    let arch = PkgFamily::Arch;
    let xorg = |b: &str| b == system::X11_SERVER_BINARY || b == "labwc";
    let both = |f: &str| f.ends_with("/xfce.desktop") || f.ends_with("/xfce-wayland.desktop");
    let x11_only = |f: &str| f.ends_with("/xfce.desktop");
    let way_only = |f: &str| f.ends_with("/xfce-wayland.desktop");
    assert_eq!(system::installed_sessions("xfce", arch, &both, &xorg), Some((true, true)));
    assert_eq!(system::installed_sessions("xfce", arch, &x11_only, &xorg), Some((true, false)));
    assert_eq!(system::installed_sessions("xfce", arch, &way_only, &xorg), Some((false, true)));
    // Without an X server the X11 session does not start, even if the file is there.
    assert_eq!(system::installed_sessions("xfce", arch, &both, &|b| b == "labwc"), Some((false, true)));
    assert_eq!(system::installed_sessions("kde", arch, &both, &xorg), None);

    assert_eq!(system::missing_session((true, true)), None);
    assert_eq!(system::missing_session((true, false)), Some(SessionKind::Wayland));
    assert_eq!(system::missing_session((false, true)), Some(SessionKind::X11));
    assert_eq!(system::missing_session((false, false)), Some(SessionKind::Wayland));

    assert_eq!(qd::session_status_line("xfce", arch, (true, true)), "Сеансы: X11 + Wayland (экспериментально)");
    assert_eq!(qd::session_status_line("xfce", arch, (true, false)), "Сеанс: X11");
    assert_eq!(qd::session_status_line("xfce", arch, (false, true)), "Сеанс: Wayland (экспериментальный)");
    assert_eq!(qd::session_status_line("lxqt", arch, (true, true)), "Сеансы: X11 + Wayland");
    assert_eq!(qd::session_status_line("lxqt", arch, (false, true)), "Сеанс: Wayland");
    assert_eq!(qd::session_status_line("kde", arch, (true, true)), "");
    // Every desktop with a choice has its X11 session file in xsessions.
    for (de, v) in system::SESSION_VARIANTS {
        assert!(v.x11_session_file.starts_with("/usr/share/xsessions/"), "{de}");
    }
}

/// A live case of 08.11.2026: GNOME was removed entirely, then a single gnome-shell was
/// installed by hand — 45 programs of the desktop are missing. The header is «Partially installed»;
/// one new package of the group (pluma-plugins in MATE) does not change the header.
#[test]
fn partial_install_changes_header_only_when_most_is_missing() {
    use rexon_core::system_plan::InstallPreview;
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let gnome = InstallPreview {
        members: s(&["gnome-shell", "gdm", "baobab", "epiphany", "gnome-calculator"]),
        new_packages: s(&["gdm", "baobab", "epiphany", "gnome-calculator"]),
        ..InstallPreview::default()
    };
    assert!(qd::is_partial_install(&gnome));
    assert_eq!(qd::packages_view(true, Some(&gnome), 5, 0).status_override, qd::status_partial());
    // Not installed — its own header («Not installed»), no replacement.
    assert!(qd::packages_view(false, Some(&gnome), 5, 0).status_override.is_empty());

    let mate = InstallPreview {
        members: s(&["caja", "marco", "mate-panel", "pluma", "pluma-plugins"]),
        new_packages: s(&["pluma-plugins"]),
        ..InstallPreview::default()
    };
    assert!(!qd::is_partial_install(&mate));
    assert!(qd::packages_view(true, Some(&mate), 5, 0).status_override.is_empty());
}
