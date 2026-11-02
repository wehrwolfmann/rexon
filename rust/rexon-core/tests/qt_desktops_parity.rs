//! Сверка `qt_desktops.rs` — вкладки «Рабочие столы».
//!
//! Эталон здесь — строки и числа, выписанные в тесты дословно. Когда-то они
//! были выписаны из виджетной вкладки `rexon/qt_desktops/window.py`; с
//! 02.10.2026 Python в Rexon — только архив, и эти записи — собственный
//! эталон Rust: любой их сдвиг в ядре красит сверку, пока эталон не поправлен
//! осознанно.
//!
//! Отдельно проверяются ЗАЩИТЫ УДАЛЕНИЯ: удалить текущий стол и удалить
//! последний установленный нельзя ни при каких входных данных. Это то, что
//! стоит человеку графической сессии, поэтому проверяется матрицей, а не
//! одним случаем (правило #13).
//!
//! Осознанные отличия от прежней виджетной вкладки:
//!   1. подсказка «Недоступен на этом дистрибутиве» больше не глотается,
//!      когда проба доступности бросает исключение — у ядра пробы нет,
//!      ответ вычисляется таблицей;
//!   2. цвета статусов — в шестнадцатеричной записи (`#f59e0b` вместо
//!      `rgba(245,158,11,1)`): QML не понимает QSS-обёртку, а цвет тот же.

mod i18n_pin;

use rexon_core::appdefaults;
use rexon_core::qt_compare as qc;
use rexon_core::qt_desktops as qd;
use rexon_core::system::{self, PkgFamily, DE_META};

// ── Список слева ────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Состав секции «Тайловые» продублирован в двух местах: продуктовый выбор
/// «кого вынести и в каком порядке» — здесь, а «тайловый ли это композитор»
/// умеет решать `appdefaults`. Разъедутся — покупатель увидит новый тайловый
/// WM в «Классических», и заметить это можно только глазом.
///
/// Сверка идёт строго по известным ядру столам: на неизвестные id
/// `is_tiling_wm` отвечает `true`, но таких в базе не бывает.
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
    // Hyprland первым как самый популярный — порядок продуктовый, ядро его
    // не знает.
    assert_eq!(qd::TILING_IDS, &["hyprland", "niri", "sway", "river"]);
}

#[test]
fn item_text_is_icon_and_name() {
    // Два пробела между значком и именем — как в оригинале
    // (`f"{icon}  {de.display_name}"`).
    assert_eq!(qd::item_text("kde"), format!("{}  KDE Plasma", qc::de_icon("kde")));
    assert_eq!(qd::item_text("xfce"), format!("{}  XFCE", qc::de_icon("xfce")));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn item_tooltip_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::item_tooltip("xfce", false, true), "");
    assert_eq!(qd::item_tooltip("xfce", true, true), "Установлен");
    assert_eq!(
        qd::item_tooltip("xfce", false, false),
        "Недоступен на этом дистрибутиве"
    );
    // COSMIC — единственный стол со своей подсказкой: гибрид.
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn list_rows_split_into_three_sections() {
    let _lang = i18n_pin::ru();
    let rows = qd::list_rows(PkgFamily::Arch, &|_| false);
    let sections: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == "section")
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(sections, vec!["Классические", "Тайловые", "Композиторы с эффектами"]);

    // Тайловые идут в своём порядке и ТОЛЬКО после заголовка секции.
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
    // Wayfire — после «Тайловых», своей секцией: окна плавающие, но это не
    // полный рабочий стол.
    assert_eq!(ids(&rows[effects_start + 1..]), vec!["wayfire"]);

    // COSMIC остаётся в «Классических» — он полноценный гибрид.
    let classic = ids(&rows[..tiling_start]);
    assert!(classic.iter().any(|c| c == "cosmic"), "COSMIC уехал из «Классических»");
    assert!(!classic.iter().any(|c| c == "wayfire"), "Wayfire вернулся в «Классические»");

    // В списке ровно столько столов, сколько в базе — ни один не потерян.
    let total = rows.iter().filter(|r| r.kind == "de").count();
    assert_eq!(total, DE_META.len());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn list_rows_carry_installed_flag() {
    let _lang = i18n_pin::ru();
    let rows = qd::list_rows(PkgFamily::Arch, &|id| id == "kde");
    let kde = rows.iter().find(|r| r.de_id == "kde").expect("KDE в списке");
    assert!(kde.installed);
    assert_eq!(kde.tooltip, "Установлен");
    let xfce = rows.iter().find(|r| r.de_id == "xfce").expect("XFCE в списке");
    assert!(!xfce.installed);
}

// ── Панель «Инструменты» ────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn tool_buttons_match_python() {
    let _lang = i18n_pin::ru();
    // Где копию панели снимать нечем — кнопок панели нет.
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

    // KDE, MATE, XFCE — одни и те же кнопки с общими подписями, без «KDE».
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
    assert!(!qd::panel_restore_confirm_text().contains("KDE"));
    assert!(!qd::panel_backup_result(true, "x").text.contains("KDE"));
    assert!(!qd::panel_restore_result(true, "x").text.contains("KDE"));
    // Чем снимается копия в каком окружении.
    let have = |t: &str| ["mate-panel", "dconf", "xfce4-panel-profiles"].contains(&t);
    for de in ["kde", "mate", "xfce"] {
        assert!(qd::panel_backup_supported(de, &have), "{de}");
    }
    assert!(!qd::panel_backup_supported("xfce", &|_| false), "без xfce4-panel-profiles нечем");
    assert!(!qd::panel_backup_supported("gnome", &have));
    // «Заменить копию?» про «Вернуть прежнюю панель» — только у MATE.
    assert!(qd::panel_backup_replace_confirm(true).1.contains("Вернуть прежнюю панель"));
    assert!(!qd::panel_backup_replace_confirm(false).1.contains("Вернуть прежнюю панель"));
    assert_eq!(qd::tools_header(), "🧰 Инструменты");
    assert_eq!(qd::TOOL_BUTTON_MIN_HEIGHT, 40);
}

// ── Карточка ────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn status_view_matches_python() {
    let _lang = i18n_pin::ru();
    let installed = qd::status_view(true, true);
    assert_eq!(installed.text, "✅ Установлен");
    assert_eq!(installed.color, "#4ade80");
    assert!(installed.bold);

    let missing = qd::status_view(false, true);
    assert_eq!(missing.text, "❌ Не установлен");
    // Оригинал красил эту строку обычным вторичным текстом.
    assert_eq!(missing.color, "");
    assert!(!missing.bold);

    let unavailable = qd::status_view(false, false);
    assert_eq!(unavailable.text, "⚠️ Недоступен");
    // Тот же цвет, что `rgba(245,158,11,1)` оригинала, в записи для QML.
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
        // Оценка — как в «Сравнении»: значки по оценке и «☆» до пяти, так
        // что пустые места видны и колонка никогда не пустая.
        assert_eq!(cells[2].icon, rexon_core::qt_compare::stars(meta.customizable, "⭐"));
        assert_eq!(cells[2].icon.chars().count(), 5);
        assert_eq!(cells[2].label, "Настройка");
        assert_eq!(cells[3].icon, rexon_core::qt_compare::stars(meta.gaming_friendly, "🎮"));
        assert_eq!(cells[3].icon.chars().count(), 5);
        assert_eq!(cells[3].label, "Игры");
        // Пустое значение оригинал показывал прозрачной точкой.
        assert_eq!(cells[2].value, "");
        assert_eq!(cells[3].value, "");
    }
    assert_eq!(qd::STAT_EMPTY_VALUE, "·");
    assert!(qd::stats("нет-такого-стола").is_empty());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn description_drops_leading_emoji() {
    let _lang = i18n_pin::ru();
    // Заголовок карточки значок уже несёт: второй раз он не нужен.
    for meta in DE_META {
        let text = qd::description(meta.id);
        let first = text.split(' ').next().unwrap_or("");
        assert!(
            first.chars().any(|c| c.is_alphanumeric()),
            "описание «{}» начинается со значка: {text}",
            meta.id
        );
    }
    // Cinnamon в базе начинается с 🍃 — именно его и стрипаем.
    assert_eq!(
        qd::description("cinnamon-mint"),
        "Рабочий стол Cinnamon с полным набором компонентов Linux Mint"
    );
    // Описание без значка не режется.
    assert_eq!(
        qd::description("xfce"),
        "Лёгкий и быстрый рабочий стол с низким потреблением ресурсов"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Блок «пакеты» — честно по состоянию стола (08.10.2026: у установленного
/// COSMIC стояло «К установке 29 пакетов», у Pantheon считались имена, а не
/// раскрытая группа). Ответ пробной транзакции подставной.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn packages_view_is_honest() {
    use rexon_core::system_plan::InstallPreview;
    let _lang = i18n_pin::ru();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();

    // Не установлен: число — что РЕАЛЬНО поставится; группа раскрыта.
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

    // Установлен, всё на месте: никакого «к установке».
    let full = InstallPreview { members: s(&["a", "b", "c"]), ..InstallPreview::default() };
    let v = qd::packages_view(true, Some(&full), 1, 0);
    assert_eq!(v.text, "✅ Все 3 пакета этого рабочего стола на месте");
    assert!(v.notice.is_empty());

    // Установлен, часть пропала.
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

    // Установлен, пропаж нет, но для части пакетов вышли новые сборки
    // (19.10.2026, MATE: mate-applets и mate-netbook) — это не «не хватает».
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

    // И пропажа, и обновления: «не хватает» — только не установленные.
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

    // Без ответа пробы: установленному — состав набора, а не «к установке».
    let v = qd::packages_view(true, None, 29, 0);
    assert_eq!(v.text, "📦 Набор рабочего стола: 29 пакетов");
    assert_eq!(
        qd::packages_view(false, None, 3, 0).text,
        "📦 К установке 3 пакета  (все из официальных репозиториев)"
    );

    // Сломано то, без чего стола нет — отказ.
    let blocked = InstallPreview { blocked: s(&["pantheon-session"]), ..InstallPreview::default() };
    assert!(qd::packages_notice(&blocked).starts_with("⛔ Установка сейчас не пройдёт"));

    // Строка версии у установленного стола.
    assert_eq!(qd::installed_version_line("1.0.3", "1.0.3", Some(false)), "Установлена версия 1.0.3");
    assert_eq!(
        qd::installed_version_line("1.0.3", "1.0.5", Some(true)),
        "Установлена версия 1.0.3, доступна 1.0.5"
    );
    assert_eq!(qd::installed_version_line("", "1.0.5", None), "");
}

/// Конфликт набора с установленным (живой случай 19.10.2026: Deepin на
/// машине с KDE Plasma — deepin-kwin против kwin). Карточка и отказ до
/// пароля объясняют по-человечески; удалять чужой стол Rexon не берётся.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
    // Чей пакет — неизвестно: называем пакеты, без выдуманных столов.
    assert_eq!(
        conflict_explanation(&[c("kate", "kwrite")]),
        "⛔ Установка не пройдёт: kate конфликтует с установленным пакетом kwrite. \
         pacman пришлось бы удалить kwrite, а сам Rexon этого не делает."
    );
    // Неважно, у кого объявлен конфликт: стол назван по установленному.
    assert_eq!(
        conflict_explanation(&[c("power-profiles-daemon", "tuned-ppd")]),
        "⛔ Установка не пройдёт: power-profiles-daemon конфликтует с установленным пакетом \
         tuned-ppd. pacman пришлось бы удалить tuned-ppd, а сам Rexon этого не делает."
    );
}

/// Живой случай 20.10.2026 (MATE, рядом Budgie): Pantheon нужен mutter49, а
/// mutter 51 держит Budgie. Называем столы и версии, а не «pacman пришлось
/// бы удалить mutter»; не стол — называем, кто требует пакет.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Конфликт с установленным (живой случай 20.10.2026: Deepin и Pantheon
/// рядом с Budgie/KDE): «Установить» под ⛔-текстом видна, но неактивна, а
/// «Собрать заново из исходников Arch» не предлагается — пересборка пакета
/// конфликта не снимает. Без конфликта всё как было.
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

    // Неактивны ровно кнопки, которые ставят набор стола.
    let mut kinds: Vec<&str> = Vec::new();
    for (installed, current) in [(false, false), (true, false), (true, true)] {
        kinds.extend(qd::card_buttons(installed, true, current).iter().map(|b| b.kind));
    }
    kinds.sort_unstable();
    kinds.dedup();
    let off: Vec<&str> = kinds.iter().copied().filter(|k| qd::installs_desktop_set(k)).collect();
    assert_eq!(off, ["install", "repair"]);
}

/// Кнопки карточки по состоянию стола — с ними же и первая защита.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn card_buttons_match_python() {
    let _lang = i18n_pin::ru();
    // Недоступен на дистрибутиве — кнопок нет вовсе, только объяснение.
    assert!(qd::card_buttons(false, false, false).is_empty());
    assert!(qd::card_buttons(true, false, false).is_empty());

    // Не установлен — одна кнопка установки, выделенная.
    let fresh = qd::card_buttons(false, true, false);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].kind, "install");
    assert_eq!(fresh[0].text, "📦 Установить");
    assert_eq!(fresh[0].style, "primary");

    // Установлен и это НЕ текущий стол — три кнопки в порядке оригинала.
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

/// Кнопки карточки уходят в разметку словарём: ключи и значения вида, по
/// которым окно выбирает обработчик и таблицу стилей, — закрытый набор.
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

/// ⛔ Первая защита: у ТЕКУЩЕГО стола кнопок «Переключиться» и «Удалить»
/// нет вовсе. Нажать нечего — значит и остаться без графики нельзя.
#[test]
fn current_desktop_has_no_switch_and_no_remove() {
    let current = qd::card_buttons(true, true, true);
    let kinds: Vec<&str> = current.iter().map(|b| b.kind).collect();
    assert_eq!(kinds, vec!["repair"], "у текущего стола лишние кнопки: {kinds:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn version_line_matches_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::version_checking(), "Проверяю версию…");
    assert_eq!(
        qd::version_line("6.1.2-1"),
        "Доступно в вашем дистрибутиве: 6.1.2-1"
    );
    // Версии нет — строка скрывается, а не показывает пустоту.
    assert_eq!(qd::version_line(""), "");
}

// ── Карусель ────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
    // Маленькое окно: работает нижний порог.
    assert_eq!(qd::carousel_height(600, 400), 320);
    // Развёрнутый Full HD: 55% окна, но не выше 720.
    assert_eq!(qd::carousel_height(1080, 1600), 594);
    assert_eq!(qd::carousel_height(2000, 3000), 720);
    // Узкая колонка: идеал 16:9 ниже потолка.
    assert_eq!(qd::carousel_height(1080, 700), 393);
}

/// ⭐ Рамка снимка отдаёт высоту карточке, когда окну её не хватает.
///
/// ЧТО ЛОВИТ. На штатном окне 1100×750 подвал состояния (34 точки) срезал
/// нижний ряд кнопок карточки: вьюпорт становился 642 точки, а карточке
/// нужно 666, у hyprland и niri — 687. Рамка при этом стояла на своём поле
/// 320 и не уступала ни точки, потому что 16:9 от её ширины (553 точки)
/// даёт 311 — меньше пола. Сторож держит и новое поведение, и обе его
/// границы: без нужды рамка не ужимается, а ниже предела не опускается.
#[test]
fn carousel_frame_yields_height_to_card_buttons() {
    // Замер 19.09.2026, штатное окно 1100×750, карточка шириной 569.
    let full = qd::carousel_height(676, 569 - 2 * qd::CAROUSEL_FRAME_PADDING_H);
    assert_eq!(full, qd::CAROUSEL_MIN_HEIGHT, "рамка стоит на своём поле");

    // Высоты хватает — рамка не трогается.
    assert_eq!(qd::carousel_height_fitted(full, 330), full);
    assert_eq!(qd::carousel_height_fitted(full, full), full);

    // Подвал состояния забрал 34 точки: рамке остаётся 296, их она и
    // отдаёт — ровно столько, сколько не хватает, не больше.
    assert_eq!(qd::carousel_height_fitted(full, 296), 296);
    // Худшая из одиннадцати карточек (hyprland, niri): описание длиннее на
    // строку, рамке остаётся 275.
    assert_eq!(qd::carousel_height_fitted(full, 275), 275);

    // Предел: дальше рамка не уступает, даже если просят больше.
    assert_eq!(qd::CAROUSEL_MAX_SQUEEZE, 72);
    let floor = full - qd::CAROUSEL_MAX_SQUEEZE;
    assert_eq!(qd::carousel_height_fitted(full, floor - 1), floor);
    assert_eq!(qd::carousel_height_fitted(full, 0), floor);
    assert_eq!(qd::carousel_height_fitted(full, -100), floor);
    assert!(floor > 0, "рамка не имеет права исчезнуть: {floor}");

    // Разметка спрашивает ядро, а не считает сама, и ширину рамки берёт от
    // НЕужатой высоты: посчитай её от ужатой — и выйдет «храповик».
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

/// ⭐ «Увеличить во весь экран» открывает ОКНО во весь экран.
///
/// ЧТО ЛОВИТ. Увеличение снимка рисовалось `Popup`'ом размером во вкладку:
/// поверх картинки оставались полоса разделов и подвал состояния, а поля
/// вокруг были полями страницы — то есть подпись кнопки обещала одно, а
/// показывала другое. У виджетного оригинала это `QDialog` без рамки,
/// модальный, с `showFullScreen()` (`rexon/qt_desktops/window.py`).
///
/// Сторож читает разметку и держит все четыре условия: окно верхнего
/// уровня, полный экран с запасным размером по экрану, модальность и выход
/// по Esc. Esc назван отдельно: у `Popup` его закрывал `closePolicy`, у
/// окна такого свойства нет, и при переносе клавиша молча пропала бы.
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

    // Запасной размер: `showFullScreen()` — просьба к оконному менеджеру, и
    // там, где её не исполняют, окно осталось бы размером со своё
    // содержимое (замер на стенде без оконного менеджера: 160×150).
    assert!(QML.contains("zoom.width = Screen.width;"), "нет запасного размера по экрану");
    assert!(QML.contains("zoom.height = Screen.height;"), "нет запасного размера по экрану");

    // Выход по Esc и одна точка выхода на все четыре пути: щелчок по
    // снимку, крестик, Esc и Q.
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

// ── Тексты операций ─────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
    // Обычный итог — выход из сеанса: перезагрузка столу не нужна (живая
    // установка River 24.10.2026 звала перезагружаться зря).
    assert_eq!(
        t.done_text,
        "<b>KDE Plasma</b> установлен.\n\
Выйдите из сеанса и выберите KDE Plasma на экране входа."
    );
    // Перезагрузка — только когда исполнитель так решил по плану установки
    // (ядро, модуль ядра, впервые включённый экран входа).
    assert_eq!(
        t.done_text_restart,
        "<b>KDE Plasma</b> установлен.\n\
Перезагрузите компьютер и выберите этот рабочий стол на экране входа."
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Тексты переключения у обеих вкладок дословно одни. Своей копии вкладка
/// «Рабочие столы» не заводит — берёт `qt_compare::switch_texts`; этот тест
/// сторожит, что там они и остались.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn repair_current_unknown_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::repair_current_unknown_title(), "Рабочий стол не определён");
    assert_eq!(
        qd::repair_current_unknown_text(),
        "Не удалось определить текущий рабочий стол — выберите его в списке слева \
и нажмите «Проверить / Восстановить» на карточке."
    );
}

// ── ⛔ Защиты удаления ──────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// Матрица обеих защит по ВСЕМ столам базы: удаление разрешается ровно
/// тогда, когда стол не текущий и есть хотя бы один другой установленный.
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
                // Отказ ВСЕГДА объясняет, что делать: молчаливая кнопка
                // здесь хуже, чем отказ.
                if !plan.allowed() {
                    assert!(!plan.text.is_empty());
                    assert!(!plan.title.is_empty());
                }
            }
        }
    }
}

// ── Копия панели (KDE, MATE, XFCE) ─────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn panel_texts_match_python() {
    let _lang = i18n_pin::ru();
    assert_eq!(qd::panel_restore_confirm_title(), "Вернуть панель?");
    assert_eq!(
        qd::panel_restore_confirm_text(),
        "Вернуть панель из последней сохранённой копии?\n\n\
Текущие настройки панели будут заменены сохранёнными."
    );

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

// ── Общие сторожа ───────────────────────────────────────────────────────

/// У каждого стола базы есть имя и описание: карточка без них выглядела бы
/// поломкой продукта, а не «нет данных».
#[test]
fn every_desktop_has_name_and_description() {
    for meta in DE_META {
        assert!(!qd::display_name(meta.id).is_empty(), "{}", meta.id);
        assert!(!qd::description(meta.id).is_empty(), "{}", meta.id);
    }
    // Неизвестный стол показывается своим идентификатором, а не пустотой.
    assert_eq!(qd::display_name("нет-такого"), "нет-такого");
}

/// Строка «Доступно в твоём дистрибутиве» обязана заказываться тогда, когда
/// у страницы появился объект побочных действий.
///
/// Хозяин кладёт его свойством уже ПОСЛЕ того, как разметка собралась: в
/// `Component.onCompleted` он ещё null, и запрос версии оттуда уходит в
/// пустоту. Без обработчика `onActionsChanged` строка не появлялась никогда
/// — вкладка молча теряла ответ пакетного менеджера. Сторож читает саму
/// разметку: проверить это на текстах ядра нельзя.
#[test]
fn version_is_requested_when_actions_arrive() {
    let qml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rexon-gui/qml/DesktopsPage.qml"
    );
    let qml = std::fs::read_to_string(qml_path).expect("нет разметки DesktopsPage.qml");
    // Обработчик спрашивает и версию, и состав пакетов (пробная транзакция
    // ходит через тот же объект действий и так же теряется без него).
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

// ── Краски и размеры разметки ───────────────────────────────────────────

/// Числа таблицы стилей оригинала, выписанные сюда дословно: QML красится
/// ими через ядро, и любой сдвиг в ядре красит сверку.
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
    // Скругления в QSS: 20 у стрелки, 22 у лупы, 24 у кнопок просмотра.
    assert_eq!(qd::round_radius(qd::ARROW_SIZE), 20);
    assert_eq!(qd::round_radius(qd::ZOOM_BUTTON_SIZE), 22);
    assert_eq!(qd::round_radius(qd::ZOOM_ARROW_SIZE), 24);
    assert_eq!(qd::ZOOM_BG_QSS, "rgba(0,0,0,0.95)");
    assert_eq!((qd::ZOOM_MARGIN, qd::ZOOM_ARROW_SIZE, qd::ZOOM_ARROW_POINT_SIZE), (16, 48, 18));
    assert_eq!(qd::ZOOM_CLOSE_POINT_SIZE, 26);
    assert_eq!(qd::ZOOM_HINT_QSS, "rgba(255,255,255,0.6)");
    assert_eq!(qd::ZOOM_HINT_PADDING, 8);
}

/// Потолок правой панели — формула `_sync_detail_width` оригинала:
/// `h = max(320, min(0.55·окно, 720))`, `ideal_w = h·16/9 + 16`,
/// потолок = 12 + ideal_w + 24 + 20.
#[test]
fn detail_panel_max_width_matches_python_formula() {
    for (window_h, want) in [(600, 12 + (330 * 16 / 9 + 16) + 44), (400, 12 + (320 * 16 / 9 + 16) + 44), (2000, 12 + (720 * 16 / 9 + 16) + 44)] {
        assert_eq!(qd::detail_panel_max_width(window_h), want, "окно {window_h}");
    }
    assert_eq!(qd::detail_panel_max_width(6200), 1352);
}

/// QSS-краски переводятся в QML-запись без потерь: то, чем красится
/// страница, и то, что выписано из оригинала, — один и тот же цвет.
#[test]
fn qss_colors_translate_for_qml() {
    use rexon_core::ui_base::{css_color_to_qml, neutral_rgba};
    // ⚠️Доли усечены, как их усекает разбор таблиц стилей Qt: 0.4 → 0x66,
    // 0.85 → 0xd8, 0.75 → 0xbf, 0.55 → 0x8c, 0.6 → 0x99, 0.25 → 0x3f,
    // 0.05 → 0x0c. Округление давало краску на ступень гуще виджетной.
    assert_eq!(css_color_to_qml(qd::LIST_SELECTED_QSS), "#663b82f6");
    assert_eq!(css_color_to_qml(qd::PRIMARY_BUTTON_QSS), "#d83b82f6");
    assert_eq!(css_color_to_qml(qd::PRIMARY_BUTTON_HOVER_QSS), "#ff3b82f6");
    assert_eq!(css_color_to_qml(qd::DESTRUCTIVE_BUTTON_QSS), "#bfdc5050");
    assert_eq!(css_color_to_qml(qd::OVERLAY_BUTTON_QSS), "#8c000000");
    assert_eq!(css_color_to_qml(qd::ZOOM_HINT_QSS), "#99ffffff");
    assert_eq!(css_color_to_qml(&neutral_rgba(qd::SEPARATOR_ALPHA)), "#3f7f7f7f");
    assert_eq!(css_color_to_qml(&neutral_rgba(qd::TOOLS_PANEL_BG_ALPHA)), "#0c7f7f7f");
}

/// Цвет заголовка секции: роль PlaceholderText скина, а у системного скина
/// — вторичный текст с прозрачностью плана палитры.
#[test]
fn placeholder_text_color_follows_skin() {
    use rexon_core::ui_base::placeholder_text_color;
    assert_eq!(placeholder_text_color("steam_dark"), "#9fb4c6");
    assert_eq!(placeholder_text_color("light"), "#7a8086");
    assert_eq!(placeholder_text_color("system"), "#beffffff");
}

/// Делёж ширины окна между панелями — ровно как у QSplitter оригинала.
///
/// ⚠️ ЧИСЛА НЕ ВЫВЕДЕНЫ, А СНЯТЫ. Замер 05.09: виджетная вкладка строилась
/// семнадцать раз на разной ширине окна (высота 6400, скин steam_dark), и
/// после показа читался `QSplitter.sizes()`. Оттуда обе поправки к прежней
/// формуле: рукоятки забирают своё ДО дележа, а лишнее место делится
/// пропорционально стартовым ширинам 170:800, а не растяжке 1:4. Прежняя
/// формула ошибалась на окне стенда на 8 точек, и вся панель вместе с
/// разделителем уезжала вправо.
#[test]
fn split_widths_follow_splitter_stretch() {
    assert_eq!(qd::DETAIL_INITIAL_WIDTH, 800);
    assert_eq!((qd::SPLIT_STRETCH_TOOLS, qd::SPLIT_STRETCH_DETAIL), (1, 4));
    assert_eq!((qd::SPLIT_HANDLE_WIDTH, qd::SPLIT_HANDLE_COUNT), (1, 2));

    // Снято с оригинала: (ширина окна, список, инструменты, карточка).
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
        // Панели плюс рукоятки — ровно ширина окна, без щелей и нахлёста.
        assert_eq!(
            list + tools + detail + qd::SPLIT_HANDLE_WIDTH * qd::SPLIT_HANDLE_COUNT,
            total,
            "окно {total}: панели не сошлись с шириной окна"
        );
    }

    // Наименьшая ширина панели не пробивается даже на самом узком окне.
    assert!(qd::split_widths(960, 6400).1 >= qd::TOOLS_MIN_WIDTH);
    // Потолок карточки остаётся за `detail_panel_max_width` при любой высоте.
    let (list, tools, detail) = qd::split_widths(2400, 1080);
    assert_eq!(list, 240);
    assert_eq!(detail, qd::detail_panel_max_width(1080));
    assert_eq!(
        list + tools + detail + qd::SPLIT_HANDLE_WIDTH * qd::SPLIT_HANDLE_COUNT,
        2400
    );
}

/// Пол СРЕДНЕЙ панели (с 25.10 — «Как в дистрибутиве», прежде
/// «Инструменты») считается по её содержимому, а не по одному числу:
/// подписи обязаны читаться целиком на штатном минимуме окна.
///
/// ⚠️ ЧИСЛА СНЯТЫ, А НЕ ВЫВЕДЕНЫ. Замер 18.09 на стенде (скин steam_dark,
/// окно 1100×750): у виджетного оригинала сплиттер отдавал панели ровно 160
/// точек, а сам Qt считал её наименьший размер по содержимому —
/// `tools.minimumSizeHint().width() == 165` при самой длинной кнопке 149.
#[test]
fn looks_panel_min_width_follows_its_content() {
    // Содержимое оригинала: 149 плюс поля 8+8 — ровно его 165.
    assert_eq!(qd::looks_panel_min_width(142, 149), 165);
    // Длиннее: 216 плюс те же поля.
    assert_eq!(qd::looks_panel_min_width(142, 216), 232);
    // Пол оригинала не пробивается, даже если мерить нечего.
    assert_eq!(qd::looks_panel_min_width(0, 0), qd::TOOLS_MIN_WIDTH);
    assert_eq!(qd::looks_panel_min_width(-5, -5), qd::TOOLS_MIN_WIDTH);
    // Колонка шире своего потолка панель не раздвигает.
    assert_eq!(
        qd::looks_panel_min_width(0, 900),
        qd::LOOKS_COLUMN_MAX_WIDTH + qd::TOOLS_MARGINS.0 + qd::TOOLS_MARGINS.2
    );
    // Заголовок считается наравне с колонкой, но потолок колонки на него не
    // распространяется.
    assert_eq!(qd::looks_panel_min_width(300, 100), 316);

    // Штатный минимум окна: сплиттер оригинала даёт панели 160, и подписи
    // в них не помещаются — пол по содержимому обязан быть больше.
    assert!(qd::looks_panel_min_width(142, 216) > qd::split_widths(1100, 750).1);
}

/// ⭐ «Инструменты» живут в СРЕДНЕЙ панели, справа от «Как в
/// дистрибутиве» (решение владельца 28.10.2026), а на узкой панели —
/// под ним тем же столбиком. Полосы внизу карточки больше нет.
#[test]
fn tools_sit_right_of_looks_or_below_when_narrow() {
    // Окно 1920: средняя панель ~629 точек — оба блока рядом.
    let wide = qd::split_widths(1920, 1000).1 - qd::TOOLS_MARGINS.0 - qd::TOOLS_MARGINS.2;
    assert!(qd::looks_tools_side_by_side(wide, 200, 216), "панель {wide}");
    let half = qd::looks_tools_block_width(wide, true);
    assert_eq!(half * 2 + qd::LOOKS_TOOLS_GAP, wide - (wide - qd::LOOKS_TOOLS_GAP) % 2);
    // Окно 1280: панели не хватает на два столбика — «Инструменты» под
    // «Как в дистрибутиве», каждый блок во всю ширину.
    let narrow = qd::split_widths(1280, 700).1 - qd::TOOLS_MARGINS.0 - qd::TOOLS_MARGINS.2;
    assert!(!qd::looks_tools_side_by_side(narrow, 200, 216), "панель {narrow}");
    assert_eq!(qd::looks_tools_block_width(narrow, false), narrow);
    // Ровно на границе — рядом; на точку уже — столбиком.
    let edge = 2 * 216 + qd::LOOKS_TOOLS_GAP;
    assert!(qd::looks_tools_side_by_side(edge, 150, 216));
    assert!(!qd::looks_tools_side_by_side(edge - 1, 150, 216));
    // Колонка шире потолка панель не «требует» бесконечно.
    assert!(qd::looks_tools_side_by_side(1000, 5000, 5000));
    assert_eq!(qd::looks_tools_block_width(-5, true), 0);

    // Разметка: блок «Инструменты» внутри средней панели, решает ядро,
    // карточка во всю высоту, полосы под ней нет.
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

/// Левая распорка карточки — ровно как у обёртки оригинала.
///
/// ⚠️ ЧИСЛА НЕ ВЫВЕДЕНЫ, А СНЯТЫ. Замер 05.09: виджетная вкладка строилась
/// на одиннадцати ширинах окна (высота 6000, скин steam_dark), и после
/// показа читалась геометрия обоих элементов обёртки правой панели —
/// распорки и карточки. Пока распорки в QML не было, содержимое карточки
/// начиналось на 10 точек левее оригинала.
#[test]
fn card_left_spacer_follows_wrapper_stretch() {
    assert_eq!(qd::CARD_WRAP_STRETCH, 100);
    assert_eq!(qd::CARD_MIN_WIDTH, 924);

    // Снято с оригинала: (ширина вьюпорта, распорка).
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
        // Карточка с распоркой закрывают вьюпорт целиком, без щели.
        assert!(viewport - qd::card_left_spacer(viewport) > 0);
    }

    // Пустая и отрицательная ширина распорки не дают.
    assert_eq!(qd::card_left_spacer(0), 0);
    assert_eq!(qd::card_left_spacer(-100), 0);
}


/// Рукоятка сплиттера красится долей нейтрального серого из ядра, а не
/// остаётся светло-серой от стиля QtQuick Basic.
#[test]
fn split_handle_is_painted_from_the_core() {
    use rexon_core::ui_base::{blend_css_over, css_color_to_qml};
    // Замер 05.09 на живой Qt: (73,83,96) на фоне страницы (27,40,56).
    assert_eq!(
        blend_css_over("#1b2838", qd::split_handle_qss(true)),
        "#495360"
    );
    assert_eq!((0x49, 0x53, 0x60), (73, 83, 96));
    // В QML краска уходит готовой записью `#aarrggbb`, а не строкой QSS:
    // `rgba(...)` разметка читает как недопустимый цвет и рисует чёрным.
    assert_eq!(css_color_to_qml(qd::split_handle_qss(true)), "#33ffffff");
    assert_eq!(css_color_to_qml(qd::split_handle_qss(false)), "#33000000");
}

/// ⭐ Сторож языка окна операции над столом: вопросы, отказы и итоги
/// установки, проверки, смены и удаления — простым русским для покупателя.
/// Прежние тексты говорили «DE», «logout/reboot», «через pkexec», «бэкап»;
/// следующая такая строка покраснеет здесь, а не в отзыве покупателя.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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
        // «DE» — только как отдельное слово: «KDE Plasma» не в счёт.
        assert!(
            !text.split(|c: char| !c.is_alphanumeric()).any(|w| w == "DE"),
            "в тексте для покупателя «DE» вместо «рабочий стол»: {text}"
        );
    }
}

/// ⭐ Тон кнопок окна операции над столом один на обе страницы.
///
/// Окно (`DeOperationDialog.qml`) общее у «Рабочих столов» и «Сравнения
/// DE», но краски «Да» оно берёт из палитры СТРАНИЦЫ. «Сравнение» этих
/// ключей не отдавало, и «Да» там выходила обычной кнопкой скина, а не
/// синей или красной, как на «Рабочих столах» для той же операции.
/// Сторож читает исходники: обе страницы кладут тоны одним помощником по
/// таблице ядра, своих копий ключей нет, окно просит ровно эти ключи.
#[test]
fn operation_dialog_tones_are_shared_by_both_pages() {
    const DIALOG_QML: &str = include_str!("../../rexon-gui/qml/DeOperationDialog.qml");
    const DESKTOPS_RS: &str = include_str!("../../rexon-gui/src/desktops_page.rs");
    const COMPARE_RS: &str = include_str!("../../rexon-gui/src/compare_page.rs");
    const DESKTOPS_QML: &str = include_str!("../../rexon-gui/qml/DesktopsPage.qml");
    const COMPARE_QML: &str = include_str!("../../rexon-gui/qml/ComparePage.qml");
    // Третий владелец окна — «Настройки» (доустановка недостающего).
    const SETTINGS_RS: &str = include_str!("../../rexon-gui/src/settings_page.rs");
    const SETTINGS_QML: &str = include_str!("../../rexon-gui/qml/SettingsPage.qml");
    // Четвёртый — главное окно (восстановление оформления при запуске).
    const MAIN_RS: &str = include_str!("../../rexon-gui/src/main_window.rs");
    const MAIN_QML: &str = include_str!("../../rexon-gui/qml/MainWindow.qml");

    // Краски: безопасная операция — синяя кнопка карточки, необратимая —
    // красная, те же, что у кнопок «Установить» и «Удалить».
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

    // Окно просит ровно ключи таблицы и выбирает «Да» по опасности операции.
    for key in tones.keys() {
        assert!(
            DIALOG_QML.contains(&format!("root.pal.{key}")),
            "окно операции не читает `{key}` из палитры страницы"
        );
    }
    assert!(DIALOG_QML.contains("dangerTone: root.pal.destructive_tone"));
    assert!(DIALOG_QML.contains("root.pal.primary_tone"));
    assert!(DIALOG_QML.contains("(root.op && root.op.danger) ? root.dangerTone : root.primaryTone"));

    // Обе страницы кладут тоны в палитру одним помощником и не заводят
    // своих копий ключей.
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

/// ⭐ Сторож «DE» в подписях для покупателя за пределами окна операции:
/// название сравнения (заголовок окна, шапка, подраздел), подсказка COSMIC,
/// описания столов, заметки, предупреждения и отказы совместимости
/// дистрибутивов, журнал проверок. Продукт говорит «рабочий стол» теми же
/// словами, что окно операции (`operation_texts_have_no_jargon`); новая
/// «DE» здесь покраснеет раньше, чем её увидит покупатель.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
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

/// SteamOS (Steam Deck и Steam Machine): неактивными становятся ровно те
/// кнопки, что трогают пакеты системы, — установка, доустановка, удаление и
/// «Проверить мой стол». Переключение, сравнение и копии панели остаются:
/// они пакетов не трогают и в домашней папке работают.
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

/// SteamOS: проверенные столы (Xfce, LXQt, Hyprland) ставятся и удаляются
/// образами systemd-sysext — их кнопки живые; Plasma (стол самой SteamOS) и
/// непроверенные — неактивны с причиной. Пересборку стола, в котором
/// работают сейчас, не предлагаем. Не на SteamOS — ничего не гасится.
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

/// SteamOS: вопрос установки стола-образа — текст `steamos_sysext` (а не
/// пакетный «выберите на экране входа»), удаление — свой вопрос.
/// Прочие столы и не-SteamOS — обычные тексты.
#[test]
fn steamos_sysext_desktops_get_their_own_texts() {
    use rexon_core::steamos_sysext as sx;
    let base = qd::install_texts("Xfce");
    let t = qd::steamos_op_texts(qd::install_texts("Xfce"), "xfce", Some("3.9.5"));
    assert_eq!(t.confirm_html, sx::install_confirm_text("Xfce", "3.9.5"));
    // Итог — строкой исполнителя в журнале, приписки под журналом нет.
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

// ── Английский интерфейс ────────────────────────────────────────────────

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все подписи вкладки, собранные ядром, при английском языке — без единой
/// русской буквы: забытая строка видна здесь, а не покупателю.
///
/// Описания столов (`DeMeta::description_text`) и тексты переключения
/// (`qt_compare::switch_texts`) переводит заход «Сравнения» — их сторожит
/// его сверка, здесь они не собираются.
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
        qd::panel_restore_confirm_text(),
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
        // Тот же сторож жаргона, что у русского окна операции.
        for jargon in ["pkexec", "polkit", "install_de"] {
            assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
        }
        assert!(
            !text.split(|c: char| !c.is_alphanumeric()).any(|w| w == "DE"),
            "«DE» вместо «desktop»: {text}"
        );
    }
}

/// Английские фразы, собранные из частей, читаются как фразы.
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

// ── Блок «Как в дистрибутиве» ───────────────────────────────────────────

mod looks {
    use super::*;
    use rexon_core::distro_looks::{self as dl, Probes, Target};
    use std::collections::BTreeMap;

    /// Стол без единой записи на диск: план собирается из проб, профиль не
    /// нужен вовсе (каталоги указывают в несуществующее).
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

    /// Предложение «Как в SteamOS» на Plasma. `aur_missing` — пакетов
    /// игрового режима из AUR нет (их нет и в репозиториях); иначе стоит всё.
    fn steamos_offer(aur_missing: bool, applied: &dyn Fn(&str) -> bool) -> dl::Offer {
        let missing = move |n: &str| aur_missing && n.contains("gamescope-session");
        let owner = |n: &str| (!missing(n)).then(|| n.to_string());
        let in_repos = |n: &str| !missing(n);
        let no_ext = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &in_repos, extension_installed: &no_ext };
        dl::offer(dl::builtin("steamos").unwrap().unwrap(), &target("kde"), &probes, applied)
    }

    /// Варианты по умолчанию.
    fn def() -> rexon_core::rxn_manifest::PropValues {
        Default::default()
    }

    fn vgui() -> rexon_core::rxn_manifest::PropValues {
        [("variant".to_string(), serde_json::Value::String("vgui".into()))].into_iter().collect()
    }

    /// Стиль SteamOS — переключатель окна вопроса: Vapor по умолчанию и
    /// VGUI2, подписи из манифеста. VGUI2 тянет классические курсоры KDE
    /// (oxygen-cursors) — их нет, и переключение сразу меняет и текст, и
    /// план, без новых проб системы.
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
        // Выбор стиля и игровой режим — независимы.
        assert!(qd::look_chosen_plan(&offer, &vgui(), true).unwrap().items.iter().any(|i| i.kind == "game-session"));
        // У Garuda списков нет — и переключателей нет.
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

    /// Плитка на каждый встроенный образ, имя — из манифеста; у
    /// применённого — пометка и «Вернуть как было», у прочих — «Применить».
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
        // Русские имена — тоже из манифеста, а не из каталога перевода.
        assert_eq!(qd::look_tiles("ru", &|_| false)[0].name, "Как в SteamOS");
    }

    /// Игровой режим SteamOS — только по желанию: без галочки его нет ни в
    /// «что изменится», ни в «не будет применено», а в применение идёт план
    /// без него; с галочкой — план с ним. Под галочкой — предупреждение из
    /// манифеста.
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

    /// Игровому режиму нужны пакеты из AUR, а их нет: с галочкой окно
    /// честно говорит «не будет применено» и называет пакеты — Rexon их сам
    /// не собирает; остальное оформление применяется.
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
        // Без галочки про AUR ни слова: человек его не просил.
        let (_, plain, _) = ask(qd::look_confirm(&offer, &def(), false));
        assert!(!plain.contains("AUR"), "{plain}");
    }

    /// Не встаёт обязательное (libplasma подменена форком) — окно отказа,
    /// применять нечего.
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

    /// Другой образ уже применён — перед этим он вернётся как было, и окно
    /// говорит об этом первым делом.
    #[test]
    fn applied_other_look_is_put_back_first() {
        let _lang = i18n_pin::en();
        let offer = steamos_offer(false, &|id| id == "garuda-dr460nized" || id == "steamos");
        assert_eq!(offer.replaces.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["garuda-dr460nized"]);
        let (_, text, _) = ask(qd::look_confirm(&offer, &def(), false));
        assert!(text.contains("First, “Like Garuda Linux (Dr460nized)” will be put back as it was."), "{text}");
    }

    /// Русские подписи блока — из `po/ru.po`.
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

    /// Все подписи блока по-английски — без единой русской буквы.
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
