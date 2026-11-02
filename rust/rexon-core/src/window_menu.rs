//! Меню главного окна Rexon: состав пунктов, их подписи, доступность и все
//! тексты диалогов, которые за этими пунктами стоят.
//!
//! ЗАЧЕМ ОТДЕЛЬНЫЙ МОДУЛЬ. Каркас окна ядро уже знает — порядок разделов,
//! заголовок и размер лежат в [`crate::qt_main`]. А вот меню под кнопкой
//! «☰» знал ТОЛЬКО Python: `RexonMainWindow._build_menubar` собирал четыре
//! пункта, `_on_recover_kde`, `_on_about` и `_on_uninstall` держали при себе
//! весь человеческий текст — вопрос перед сбросом темы KDE, справку о
//! программе с атрибуцией лицензий, перечень удаляемых файлов и подписи трёх
//! кнопок удаления. В Rust-каркасе меню не было ни строки: покупатель им
//! пользуется, а хозяин на Rust его не показывал вовсе.
//!
//! Здесь живут РЕШЕНИЯ: какие пункты есть, в каком порядке, где разделитель,
//! что доступно и какими словами разговаривать. Разметка (`qml/WindowMenu.qml`,
//! `qml/WindowMenuDialogs.qml`) только рисует то, что сказано отсюда, а
//! исполнитель (`rexon-gui/src/window_menu.rs`) только запускает.
//!
//! ⚠️ ВТОРОЙ КОПИИ «ВЫХОДА» ЗДЕСЬ НЕТ. Пункт «🚪 Выход» стоит и в меню значка
//! в лотке, и его подпись со значком собирает [`crate::tray::titled`] по
//! ключу `quit`. Именно этот разъезд поймала сверка каркаса 07.09.2026
//! (окно знало «🚪 Выход», ядро — «Выход»/«Quit»), и заводить копию снова
//! запрещает правило #12.
//!
//! ЯЗЫК. Все подписи и тексты диалогов идут через каталог перевода
//! (`crate::i18n`): исходный язык — английский, русский живёт в `po/ru.po`
//! дословно таким, каким его показывало Python-окно (сверка
//! `tests/window_menu_parity.rs`). Прежние константы стали функциями:
//! константу можно забыть перевести, функцию — нет. Разбор выбора идёт по
//! идентификаторам (`ID_*`, `FORKS_BUTTON_*`), подписи код не сравнивает.

use crate::i18n::{n_, ntr, tr, trc, trf};
use crate::installer::UserDataKind;
use crate::tray;

// ============================================================================
// Кнопка и идентификаторы пунктов
// ============================================================================

/// Подпись самой кнопки меню — «гамбургер» в полосе меню окна.
/// Источник: `bar.addMenu("☰")` в `_build_menubar`.
pub const MENU_BUTTON: &str = "☰";

/// Восстановить тему KDE из резервной копии.
pub const ID_RECOVER_KDE: &str = "recover_kde";
/// Справка о программе.
pub const ID_ABOUT: &str = "about";
/// Удалить Rexon из системы. Кнопка удаления уместна: Rexon — ПРОГРАММА, а
/// не игра (в играх её не делают).
pub const ID_UNINSTALL: &str = "uninstall";
/// Выход из приложения.
pub const ID_QUIT: &str = "quit";

// ============================================================================
// Состав меню
// ============================================================================

/// Один пункт меню окна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Идентификатор — по нему хозяин окна понимает, что нажали. Ни разметка,
    /// ни исполнитель не сравнивают подписи: подпись переводится, а решение —
    /// нет.
    pub id: &'static str,
    /// Ровно та строка, которую видит покупатель.
    pub label: String,
    /// Горячая клавиша в записи Qt («Ctrl+Q»); пусто — клавиши нет.
    pub shortcut: &'static str,
    /// Перед этим пунктом стоит разделитель.
    ///
    /// Отдельным «пунктом-разделителем» список не засоряется намеренно:
    /// разметке пришлось бы иметь два вида делегата на один список, а
    /// разделитель — свойство соседства, не самостоятельный пункт.
    pub separator_before: bool,
}

/// Пункты меню В ПОРЯДКЕ МЕНЮ — единственный источник правды о составе.
///
/// Порядок и разделитель — как в `_build_menubar`: сброс темы, справка,
/// удаление, разделитель, выход. «Выход» отделён чертой потому, что он
/// единственный необратим мгновенно и без вопросов.
pub fn menu_items() -> Vec<MenuItem> {
    vec![
        MenuItem {
            id: ID_RECOVER_KDE,
            label: format!("🛟 {}", recover_title()),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            id: ID_ABOUT,
            // TRANSLATORS: item of the main window menu; the dialog it opens
            // is titled "About Rexon" without this context.
            label: format!("ℹ️ {}", trc("window menu", "About Rexon")),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            id: ID_UNINSTALL,
            label: format!("🗑️ {}", uninstall_title()),
            shortcut: "",
            separator_before: false,
        },
        MenuItem {
            // Подпись со значком берётся у лотка — второй копии нет.
            id: ID_QUIT,
            label: tray::titled("quit"),
            shortcut: "Ctrl+Q",
            separator_before: true,
        },
    ]
}

/// Пункты меню для ЭТОГО сеанса: на чужом рабочем столе без «Восстановить
/// тему KDE».
///
/// Живая проверка в MATE: пункт стоял в меню, а нажатие на него вело только
/// к отказу «работает лишь в KDE Plasma». Обещать в меню то, чего маршрут
/// заведомо не сделает, — лишняя строка и лишний вопрос у покупателя.
/// Решение то же, что у самого маршрута ([`recover_allowed`]), поэтому
/// меню и отказ не разойдутся. Полный состав по-прежнему у [`menu_items`].
pub fn menu_items_for(xdg_current_desktop: &str) -> Vec<MenuItem> {
    let recover = recover_allowed(xdg_current_desktop);
    menu_items()
        .into_iter()
        .filter(|i| recover || i.id != ID_RECOVER_KDE)
        .collect()
}

/// Доступен ли пункт меню.
///
/// В продукте недоступных пунктов нет: ни один из четырёх не заводит
/// `setEnabled(False)`, и «Удалить Rexon» открывается даже когда Rexon не
/// установлен вовсе — тогда о этом говорит сам диалог удаления
/// («Rexon не установлен»), а не серая строка меню. Вопрос всё равно
/// задаётся ядру, а не решается в разметке: как только у пункта появится
/// условие, править придётся одно место.
///
/// Незнакомый идентификатор недоступен: показать пункт, за которым ничего не
/// стоит, — это обещание, которого никто не выполнит.
pub fn is_enabled(id: &str) -> bool {
    matches!(id, ID_RECOVER_KDE | ID_ABOUT | ID_UNINSTALL | ID_QUIT)
}

/// Опасен ли пункт: за ним стоит необратимое — удаление программы.
///
/// Разметка красит такой пункт при наведении и фокусе краской опасной кнопки
/// (`destructive_tone`), а не обычной подсветкой: человек, дошедший до него
/// стрелками или геймпадом, должен видеть, что это не «О программе». Решает
/// ядро, а не разметка: сравнивать подписи нельзя (они переводятся), а
/// идентификатор в разметке — второе место, которое забудут поправить.
///
/// «Выход» сюда не входит: он закрывает окно, но ничего не стирает.
pub fn is_destructive(id: &str) -> bool {
    id == ID_UNINSTALL
}

// ============================================================================
// «О программе» (порт `_on_about`)
// ============================================================================

/// Заголовок окна справки.
pub fn about_title() -> String {
    tr("About Rexon")
}

/// Кнопка справки: открыть файл журнала.
pub fn log_open_label() -> String {
    // TRANSLATORS: button in the "About Rexon" dialog; opens the Rexon log file.
    tr("Open log")
}

/// Кнопка справки: скопировать конец журнала в буфер обмена.
pub fn log_copy_label() -> String {
    // TRANSLATORS: button in the "About Rexon" dialog; copies the end of the log to the clipboard.
    tr("Copy log")
}

/// Строка справки после копирования.
pub fn log_copied_text() -> String {
    tr("The log is copied — paste it into a message to the developer.")
}

/// Где лежит журнал и как включить подробный.
pub fn log_location_text(path: &str) -> String {
    // TRANSLATORS: %1 is the path of the log file; REXON_LOG=debug is an environment variable, keep it as is.
    trf("Log: %1 (detailed: start Rexon with REXON_LOG=debug)", &[&path])
}

/// Сколько конца журнала копировать: столько читается в сообщении глазами
/// и умещается в письмо, а последних запусков хватает для разбора.
pub const LOG_COPY_BYTES: u64 = 200 * 1024;

/// Версия продукта человеку — «0.1.1». Собирается из [`crate::constants`], а
/// не переписывается строкой: `rexon.__version__` и ядро обязаны совпадать.
pub fn version_string() -> String {
    let (major, minor, patch) = crate::constants::REXON_VERSION;
    format!("{major}.{minor}.{patch}")
}

/// Тело справки, rich text — ровно то, что показывает `QMessageBox.about`.
///
/// ⚠️ Абзац про Qt и KDE Frameworks — АТРИБУЦИЯ ЛИЦЕНЗИЙ, а не
/// украшение: обе библиотеки LGPL, и указание динамической линковки — условие
/// их использования при распространении Rexon. Правка этого абзаца — правка
/// юридического текста, не текста интерфейса.
pub fn about_html() -> String {
    format!(
        "<h2>Rexon {version}</h2>\
         <p>{tagline}</p>\
         <p>{features}</p>\
         <p>{support}</p>\
         <p>{copyright}</p>\
         <p style='color:#888;font-size:11px'>{attribution}</p>",
        version = version_string(),
        tagline = tr("Desktop environment manager for Linux"),
        features = tr("Install, remove and configure desktop environments, themes, icons, \
             cursors, fonts, wallpapers (including Wallpaper&nbsp;Engine), the login \
             screen and boot splash (SDDM/Plymouth), widgets and extensions — all in \
             one place."),
        support = tr("<b>Supported:</b> KDE, GNOME, Cinnamon, XFCE, MATE, Budgie, \
             Pantheon, Deepin, LXQt, COSMIC, Wayfire, Hyprland, sway, niri, River"),
        copyright = tr("© 2026 Wehrwolfmann · Apache License 2.0"),
        // TRANSLATORS: license attribution required by the LGPL; keep the
        // library names and license identifiers exactly as they are.
        attribution = tr("Uses Qt (LGPL-3.0) and KDE&nbsp;Frameworks \
             (LGPL-2.1+), dynamically linked."),
    )
}

// ============================================================================
// «О программе»: благодарности — сторонние программы, на которых стоит Rexon
// ============================================================================

/// Сторонняя программа, без которой не было бы части Rexon, — одна строка
/// раздела благодарностей в справке.
///
/// ЗАЧЕМ НА ВИДУ. Живые обои Rexon работают на чужом движке (waywallen,
/// Open Wallpaper Engine и др.). Файл NOTICE это фиксирует юридически, но его
/// не читает никто, кроме юристов; авторы должны быть видны покупателю там,
/// где он ищет «кто это сделал», — в «О программе».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThirdPartyComponent {
    /// Имя так, как его пишет автор. Не переводится.
    pub name: &'static str,
    /// Что эта программа делает в Rexon — словами покупателя. Переводится в
    /// месте показа (`tr(c.purpose)`), здесь только помечена для `xgettext`.
    pub purpose: &'static str,
    /// Лицензия идентификатором SPDX. Юридическая запись — не переводится.
    pub license: &'static str,
    /// Страница проекта; в справке — ссылка, открывается в браузере.
    pub url: &'static str,
    /// Начало заголовка раздела этой программы в файле NOTICE. По нему
    /// сторож (`tests/window_menu_parity.rs`) сверяет таблицу с NOTICE:
    /// раздел есть, ссылка и лицензия в нём те же.
    pub notice_heading: &'static str,
}

/// Благодарности — ЕДИНСТВЕННЫЙ список в коде. Порядок — как в разделе
/// «Credits» README. Разметка своего списка не держит: справку собирает
/// [`about_credits_html`].
pub const THIRD_PARTY: &[ThirdPartyComponent] = &[
    ThirdPartyComponent {
        name: "waywallen",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("The live wallpaper engine: it plays wallpapers on your desktop. \
                     The Live wallpapers page is built on its interface."),
        license: "MIT",
        url: "https://github.com/hypengw/waywallen",
        notice_heading: "waywallen\n",
    },
    ThirdPartyComponent {
        name: "Open Wallpaper Engine",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays Wallpaper Engine scene and web wallpapers."),
        license: "GPL-2.0-only",
        url: "https://github.com/waywallen/open-wallpaper-engine",
        notice_heading: "open-wallpaper-engine ",
    },
    ThirdPartyComponent {
        name: "waywallen-display",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Shows live wallpapers on the KDE Plasma desktop."),
        // У апстрима лицензия записана двояко (waywallen-display#70), пакет
        // везёт оба текста — и справка называет обе.
        license: "MIT / GPL-2.0-or-later",
        url: "https://github.com/waywallen/waywallen-display",
        notice_heading: "waywallen-display ",
    },
    ThirdPartyComponent {
        name: "Ruffle",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays Flash wallpapers."),
        license: "MIT OR Apache-2.0",
        url: "https://github.com/ruffle-rs/ruffle",
        notice_heading: "Ruffle\n",
    },
    ThirdPartyComponent {
        name: "QmlMaterial",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Gives the Live wallpapers page its look: buttons, lists and icons."),
        license: "MIT",
        url: "https://github.com/hypengw/QmlMaterial",
        notice_heading: "QmlMaterial ",
    },
    ThirdPartyComponent {
        name: "QExtra",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Shows wallpaper previews and animated pictures on the Live wallpapers page."),
        license: "MIT OR Apache-2.0",
        url: "https://github.com/hypengw/QExtra",
        notice_heading: "QExtra ",
    },
    ThirdPartyComponent {
        name: "Mozilla Translations",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("The language model that translates descriptions — the same kind Firefox uses."),
        license: "MPL-2.0",
        url: "https://github.com/mozilla/translations",
        notice_heading: "Модель перевода Firefox ",
    },
    ThirdPartyComponent {
        name: "fxtranslate",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Translates English descriptions of themes and wallpapers right on \
                     your computer, without sending them anywhere."),
        license: "MPL-2.0",
        url: "https://crates.io/crates/fxtranslate",
        notice_heading: "fxtranslate ",
    },
    ThirdPartyComponent {
        name: "mpv",
        // TRANSLATORS: what this third-party program does in Rexon ("About Rexon" → credits).
        purpose: n_("Plays the startup movie at login."),
        license: "GPL-2.0-or-later",
        url: "https://mpv.io",
        notice_heading: "mpv ",
    },
];

/// Раздел благодарностей справки, rich text: заголовок, пара слов зачем, по
/// строке на программу (имя — ссылка, что делает, лицензия) и куда смотреть
/// за полным списком. Ссылки открывает разметка (`onLinkActivated` →
/// `Qt.openUrlExternally`), как и все ссылки в окнах меню.
pub fn about_credits_html() -> String {
    let items: String = THIRD_PARTY.iter().map(credit_html).collect();
    format!(
        "<h3>{heading}</h3><p>{intro}</p>{items}\
         <p style='color:#888;font-size:11px'>{disclaimer}</p>\
         <p style='color:#888;font-size:11px'>{full}</p>",
        // TRANSLATORS: heading of the credits section in "About Rexon": the free programs Rexon is built on.
        heading = tr("Thanks"),
        // TRANSLATORS: intro of the credits section in "About Rexon".
        intro = tr("Live wallpapers and some other parts of Rexon are built on these \
             free programs. Thank you to their authors! Each program keeps its own license."),
        disclaimer = not_affiliated_text(),
        full = credits_full_list_text(),
    )
}

/// Отказ от связи с Wallpaper Engine и Valve. Юридическая строка, а не
/// украшение: марка «Wallpaper Engine» стоит в интерфейсе Rexon (вкладка
/// обоев), и без этой оговорки её можно принять за знак партнёрства. Текст
/// дословно тот же, что в README (раздел Credits) — сверку держит
/// `tests/window_menu_parity.rs`.
pub fn not_affiliated_text() -> String {
    // TRANSLATORS: legal notice under the credits in "About Rexon"; keep the names Rexon, Wallpaper Engine and Valve as they are.
    tr("Rexon is not affiliated with, endorsed by or sponsored by Wallpaper Engine, \
        its developers, or Valve.")
}

/// Одна строка благодарностей, rich text: имя ссылкой, что делает, лицензия.
pub fn credit_html(c: &ThirdPartyComponent) -> String {
    format!(
        "<p><b><a href='{url}'>{name}</a></b> — {purpose}<br>\
         <span style='color:#888'>{license}</span></p>",
        url = c.url,
        name = c.name,
        purpose = tr(c.purpose),
        // TRANSLATORS: "About Rexon" → credits; %1 is a license identifier such as MIT or GPL-2.0-only, keep it as is.
        license = trf("License: %1", &[&c.license]),
    )
}

/// Куда смотреть за полным списком и текстами лицензий.
pub fn credits_full_list_text() -> String {
    // TRANSLATORS: last line of the credits in "About Rexon"; NOTICE is a file name, keep it as is.
    tr("The full list of components and the license texts are in the NOTICE \
        file that comes with Rexon.")
}

/// Всё тело окна «О программе»: справка и под ней благодарности. Справку
/// [`about_html`] сверка паритета держит дословно — раздел благодарностей
/// к ней приписывается, а не вписывается внутрь.
pub fn about_dialog_html() -> String {
    format!("{}{}", about_html(), about_credits_html())
}

// ============================================================================
// «Восстановить тему KDE» (порт `_on_recover_kde`)
// ============================================================================

/// Путь резервной копии темы относительно домашней папки.
pub const RECOVER_BACKUP_REL: &str = ".config/rexon/last_kde_lookandfeel.txt";

/// Что применится, если резервной копии нет.
pub const RECOVER_FALLBACK_TARGET: &str = "Breeze Dark (fallback)";

/// Полный путь резервной копии.
///
/// Считается тем же расширителем, что и все пользовательские пути проекта
/// ([`crate::utils::expand_user`]): при нестандартном `$XDG_CONFIG_HOME`
/// вшитый `~/.config` читал бы не тот файл, и диалог называл бы не ту тему.
pub fn recover_backup_path(home: &str, xdg_config_home: Option<&str>) -> String {
    crate::utils::expand_user(
        &format!("~/{RECOVER_BACKUP_REL}"),
        home,
        xdg_config_home,
    )
}

/// Какая тема будет применена: сохранённая либо запасная.
///
/// Пустая или отсутствующая копия — это запасная тема, а не пустое имя:
/// диалог обязан назвать цель, иначе человек нажимает «да» вслепую.
pub fn recover_target(saved: Option<&str>) -> String {
    match saved.map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => RECOVER_FALLBACK_TARGET.to_string(),
    }
}

/// Заголовок вопроса перед сбросом; он же подпись пункта меню без значка.
pub fn recover_title() -> String {
    tr("Restore KDE Theme")
}

/// Вопрос перед сбросом, rich text.
///
/// ⚠️ Про обои сказано НАМЕРЕННО и жирным: сброс темы останавливает живые и
/// видео-обои, и человек, который об этом не предупреждён, примет пропавшие
/// обои за поломку Rexon (правило #13).
pub fn recover_question(target: &str) -> String {
    // TRANSLATORS: rich text; %1 is the name of the theme to apply.
    trf(
        "This theme will be applied: <b>%1</b><br><br>\
         Your <b>wallpaper will also be reset to the KDE default</b> (any \
         video or live wallpapers will stop), and the desktop will look the \
         way it did right after installing the system.<br><br>\
         Use this if an applied theme broke your system \
         (black windows, white elements, apps crashing).",
        &[&target],
    )
}

/// Имя Rust-бинаря главного окна.
pub const MAIN_WINDOW_BIN: &str = "rexon-main-window";

/// Кого звать, если свой бинарь не опознан: общий вход продукта.
pub const RECOVER_FALLBACK_BIN: &str = "rexon";

/// Хвост, который ядро Linux дописывает к `/proc/self/exe`, когда файл
/// бинаря заменили на диске после запуска (обычная пересборка).
const DELETED_EXE_SUFFIX: &str = " (deleted)";

/// Команда сброса: СВОЙ бинарь окна с флагом `--recover-kde`.
///
/// ⭐ Починка при переносе. Раньше здесь стояло `rexon --recover-kde`, то
/// есть Rust-окно отдавало аварийный откат Python-входу — хотя тот же
/// маршрут целиком живёт в ядре ([`crate::cli::run_headless`] →
/// [`crate::system_exec::SystemExec::recover_kde`]) и бинарь окна отрабатывает
/// его до подъёма Qt. Общий вход берётся, только если свой бинарь не
/// опознан (окно встроено в чужой процесс).
///
/// `own_exe` — путь своего исполняемого файла (`std::env::current_exe`).
/// Хвост « (deleted)» снимается: после пересборки по тому же пути лежит
/// уже новый бинарь, и звать надо его, а не несуществующее имя с хвостом.
pub fn recover_argv(own_exe: Option<&str>) -> Vec<String> {
    let own = own_exe
        .map(|p| p.strip_suffix(DELETED_EXE_SUFFIX).unwrap_or(p))
        .filter(|p| {
            std::path::Path::new(p).file_name().and_then(|n| n.to_str())
                == Some(MAIN_WINDOW_BIN)
        });
    vec![
        own.unwrap_or(RECOVER_FALLBACK_BIN).to_string(),
        crate::app::RECOVER_KDE_FLAG.to_string(),
    ]
}

/// Можно ли сбрасывать тему в этом сеансе. Решение то же, что у самого
/// маршрута ([`crate::app::is_plasma_desktop`]): окно не имеет права обещать
/// «восстановление запущено» там, где маршрут откажет.
pub fn recover_allowed(xdg_current_desktop: &str) -> bool {
    crate::app::is_plasma_desktop(xdg_current_desktop)
}

/// Что сказать в окне на чужом рабочем столе.
///
/// ⭐ Починка при переносе: Python запускал откат вслепую и докладывал
/// «Восстановление запущено», а маршрут на не-Plasma молча выходил с
/// кодом 1 — отказ уходил в поток вывода, которого человек не видит.
pub fn recover_not_plasma_text(xdg_current_desktop: &str) -> String {
    // TRANSLATORS: %1 is the name of the running desktop (e.g. GNOME).
    trf(
        "Theme recovery currently works only on KDE Plasma.\n\
         You are running: %1. Nothing was changed.\n\n\
         On this desktop, restore the look in the desktop's own settings \
         or on the Appearance tab.",
        &[&xdg_current_desktop],
    )
}

// ── Пакеты, подменяющие части Plasma (AeroThemePlasma / AeroShell) ─────────
//
// Порт `_maybe_handle_plasma_forks`. Такие пакеты — не тема: они заменяют
// сами программы Plasma, и сброс темы их не уберёт. Прежде чем сбрасывать,
// окно говорит об этом и предлагает вернуть обычную Plasma.
//
// Тексты сказаны проще, чем в Python: слова «форк», «сток», «pacman» и
// имена библиотек покупателю ничего не говорят (правило #13). Смысл и
// порядок шагов те же.

/// Заголовок предупреждения о пакетах-заменах.
pub fn forks_title() -> String {
    tr("Replacement Plasma Components Installed")
}

/// Текст предупреждения, rich text. Имена пакетов экранируются: они
/// приходят из пакетного менеджера, а не из этого файла.
pub fn forks_text(forks: &[String]) -> String {
    let list = forks
        .iter()
        .map(|f| format!("&bull; <code>{}</code>", crate::ui_base::escape_dialog_text(f)))
        .collect::<Vec<_>>()
        .join("<br>");
    // TRANSLATORS: rich text; %1 is the list of package names.
    trf(
        "These <b>AeroThemePlasma / AeroShell</b> packages are installed:<br><br>\
         %1<br><br>\
         They are <b>not a theme</b>: they replace parts of Plasma itself — \
         the panel, the password prompt, window management. As long as they \
         are installed, <b>resetting the theme will not remove them</b>. To \
         get standard Plasma back, they have to be removed and the standard \
         packages reinstalled.",
        &[&list],
    )
}

/// Кнопка «удалить и вернуть обычную Plasma». Кнопка по умолчанию, как в
/// продукте: за ней стоит ещё один вопрос с «Нет» по умолчанию.
pub fn forks_remove_label() -> String {
    format!("🧹 {}", tr("Remove Them and Restore Standard Plasma"))
}
/// Кнопка «только сбросить тему» — сознательная косметика поверх замен.
pub fn forks_theme_label() -> String {
    tr("Reset Theme Only")
}
/// Кнопка отмены — та же подпись, что у окна удаления.
pub fn forks_cancel_label() -> String {
    uninstall_cancel_label()
}

/// Заголовок отказа: вернуть стандартные пакеты нечем.
pub fn forks_blocked_title() -> String {
    tr("Standard Plasma Cannot Be Restored Right Now")
}

/// Заголовок последнего вопроса перед удалением.
pub fn forks_confirm_title() -> String {
    tr("Remove Replacement Plasma Components?")
}

/// Последний вопрос перед удалением, rich text: ровно та команда, что
/// выполнится (её собирает ядро), и что будет после.
///
/// Команда экранируется: в ней есть `&&`. Слова «окно может подвиснуть»
/// из Python здесь нет намеренно: удаление идёт в фоне, окно не встаёт.
pub fn forks_confirm_text(command: &str) -> String {
    let command = crate::ui_base::escape_dialog_text(command);
    // TRANSLATORS: rich text; %1 is the command that will be run.
    trf(
        "This will run (you will be asked for your password once):<br><br>\
         <code>%1</code><br><br>\
         It takes up to a minute. Afterwards you need to <b>log out and log \
         back in</b>.<br>\
         The Windows 7 style panel and widgets (SevenStart) will be gone.",
        &[&command],
    )
}

/// Заголовок сообщения «удаление идёт».
pub fn forks_running_title() -> String {
    tr("Removal in Progress")
}
/// Текст сообщения «удаление идёт» (обычный текст).
pub fn forks_running_text() -> String {
    tr("A password prompt will appear in a moment. \
        When the removal is finished, the result will be shown here.")
}

/// Заголовок итога: получилось.
pub fn forks_done_title() -> String {
    tr("Replacements Removed")
}
/// Итог «получилось» (обычный текст). Пункт меню назван дословно — его
/// подпись подставляется, а не переписывается второй копией.
pub fn forks_done_text() -> String {
    // TRANSLATORS: %1 is the name of the "Restore KDE Theme" menu item.
    trf(
        "The replacement packages have been removed and the standard Plasma \
         components restored.\n\n\
         Log out and log back in to start standard Plasma. \
         After logging in, run “%1” again if needed.",
        &[&recover_title()],
    )
}
/// Заголовок итога: не получилось. Текст — объяснение ядра
/// ([`crate::system_plan::plasma_fork_reset_outcome`]).
pub fn forks_failed_title() -> String {
    tr("Could Not Remove the Replacements")
}

/// Идентификаторы трёх кнопок окна про пакеты-замены. Разбор выбора идёт по
/// ним, а не по подписи: подпись меняется, идентификатор — нет.
pub const FORKS_BUTTON_REMOVE: &str = "remove";
pub const FORKS_BUTTON_THEME: &str = "theme";
pub const FORKS_BUTTON_CANCEL: &str = "cancel";

/// Кнопка окна про пакеты-замены: что написать, какую роль дать в ряду
/// кнопок (`accept` / `action` / `reject`) и стоит ли она по умолчанию.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksButton {
    pub id: &'static str,
    pub label: String,
    pub role: &'static str,
    pub default: bool,
}

/// Окно про пакеты-замены целиком — одно на оба окна продукта (Python и
/// Rust): своих слов и своего порядка кнопок ни у одного из них нет.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksPrompt {
    pub title: String,
    /// Rich text.
    pub text: String,
    pub buttons: Vec<ForksButton>,
}

/// Окно про пакеты-замены. `None` — замен нет, сразу вопрос о сбросе темы.
///
/// Кнопка по умолчанию — снятие замен, как в продукте: за ней стоит ещё
/// один вопрос, и там по умолчанию «Нет».
pub fn forks_prompt(forks: &[String]) -> Option<ForksPrompt> {
    if forks.is_empty() {
        return None;
    }
    let button = |id, label, role, default| ForksButton { id, label, role, default };
    Some(ForksPrompt {
        title: forks_title(),
        text: forks_text(forks),
        buttons: vec![
            button(FORKS_BUTTON_REMOVE, forks_remove_label(), "accept", true),
            button(FORKS_BUTTON_THEME, forks_theme_label(), "action", false),
            button(FORKS_BUTTON_CANCEL, forks_cancel_label(), "reject", false),
        ],
    })
}

/// Что делать после выбора в окне про пакеты-замены.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForksNext {
    /// Спросить ещё раз, показав команду, и только потом снимать.
    ConfirmRemoval,
    /// Сознательно только косметика: дальше обычный вопрос о сбросе темы.
    ThemeQuestion,
    /// Ничего не делать.
    Stop,
}

impl ForksNext {
    /// Имя шага для окон: `confirm_removal` / `theme_question` / `stop`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfirmRemoval => "confirm_removal",
            Self::ThemeQuestion => "theme_question",
            Self::Stop => "stop",
        }
    }
}

/// Следующий шаг по нажатой кнопке. Незнакомая или пустая (окно закрыли
/// Esc либо крестиком) — ничего не делать: снимать системные пакеты без
/// прямого «да» нельзя.
pub fn forks_next(button_id: &str) -> ForksNext {
    match button_id {
        FORKS_BUTTON_REMOVE => ForksNext::ConfirmRemoval,
        FORKS_BUTTON_THEME => ForksNext::ThemeQuestion,
        _ => ForksNext::Stop,
    }
}

/// Шаг после «удалить»: либо последний вопрос, либо отказ с объяснением.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForksPreviewStep {
    /// `true` — задать вопрос (`text` — rich text с командой); `false` —
    /// вернуть стандартные пакеты нечем, показать `text` обычным текстом и
    /// ничего не запускать.
    pub ok: bool,
    pub title: String,
    pub text: String,
    /// Что сказать, пока идёт снятие (пусто при отказе).
    pub running_title: String,
    pub running_text: String,
}

/// Шаг после «удалить» по ответу предпросмотра ядра
/// ([`crate::system_exec::SystemExec::plasma_fork_reset_preview`]):
/// `message` — команда при `ok`, объяснение при отказе.
pub fn forks_preview_step(ok: bool, message: &str) -> ForksPreviewStep {
    if ok {
        ForksPreviewStep {
            ok,
            title: forks_confirm_title(),
            text: forks_confirm_text(message),
            running_title: forks_running_title(),
            running_text: forks_running_text(),
        }
    } else {
        ForksPreviewStep {
            ok,
            title: forks_blocked_title(),
            text: message.to_string(),
            running_title: String::new(),
            running_text: String::new(),
        }
    }
}

/// Заголовок и текст итога удаления.
pub fn forks_finished(ok: bool, message: &str) -> (String, String) {
    if ok {
        (forks_done_title(), forks_done_text())
    } else {
        (forks_failed_title(), message.to_string())
    }
}

/// Заголовок сообщения «пошло».
pub fn recover_started_title() -> String {
    tr("Recovery Started")
}

/// Текст сообщения «пошло».
pub fn recover_started_text(target: &str) -> String {
    // TRANSLATORS: %1 is the name of the theme being applied.
    trf(
        "Applying: %1\n\n\
         KDE Plasma will switch the theme and reset the wallpaper to the \
         default; plasmashell will restart in 1–2 seconds.",
        &[&target],
    )
}

/// Заголовок сообщения о неудаче запуска.
pub fn error_title() -> String {
    tr("Error")
}

/// Текст сообщения о неудаче запуска.
pub fn recover_failed_text(error: &str) -> String {
    // TRANSLATORS: %1 is the system error message.
    trf("Could not start rexon: %1", &[&error])
}

// ============================================================================
// «Удалить Rexon» (порт `_on_uninstall` + текстов `RexonUninstaller`)
// ============================================================================

/// Заголовок окна удаления; он же подпись пункта меню без значка.
pub fn uninstall_title() -> String {
    tr("Uninstall Rexon")
}

/// Первая строка окна удаления.
pub fn uninstall_text() -> String {
    tr("Rexon's files and folders will be removed")
}

/// Сколько путей показывается списком, прежде чем остаток свернётся в счёт.
///
/// Пятнадцать — не круглое число: полный список system-установки это больше
/// сорока путей вместе с polkit-политиками хелперов, и окно вопроса уехало бы
/// за край экрана, то есть кнопки «Отмена» человек бы не увидел.
pub const UNINSTALL_FILES_SHOWN: usize = 15;

/// Перечень удаляемого списком, как его печатает окно удаления.
pub fn uninstall_files_text(files: &[String]) -> String {
    let mut out: String = files
        .iter()
        .take(UNINSTALL_FILES_SHOWN)
        .map(|f| format!("  • {f}"))
        .collect::<Vec<_>>()
        .join("\n");
    if files.len() > UNINSTALL_FILES_SHOWN {
        let rest = (files.len() - UNINSTALL_FILES_SHOWN) as u64;
        // TRANSLATORS: last line of a cut list of files: "… 3 more".
        out.push_str(&format!("\n  • … {}", ntr("%n more", "%n more", rest)));
    }
    out
}

/// Что удаляется при любом выборе — первая строка под перечнем.
pub fn uninstall_always_line() -> String {
    tr("Whichever you choose, the program itself (listed above) and Rexon's \
        temporary files are removed.")
}

/// Что добавляет «Удалить всё». Кнопка названа дословно — её подпись
/// подставляется, а не переписывается второй копией.
pub fn uninstall_delete_all_line() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf(
        "“%1” also removes Rexon's settings, desktop backups, and installed \
         wallpapers, themes and add-ons.",
        &[&uninstall_delete_label()],
    )
}

/// Предупреждение под строкой «Удалить всё»: обои и тема экрана входа,
/// которые стоят на столе прямо сейчас, лежат в папке Rexon и уходят вместе
/// с ней. Без этой фразы человек нажимает «Удалить всё» и получает
/// неожиданно голый рабочий стол (решение Wehrwolfmann 16.09.2026).
pub fn uninstall_delete_all_active_line() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf(
        "If your current wallpaper or login screen theme came from Rexon, \
         “%1” removes them too, and the desktop goes back to its standard look.",
        &[&uninstall_delete_label()],
    )
}

/// Что остаётся при «сохранить» — строка под перечнем окна удаления.
/// Прежде здесь стоял вопрос «Сохранить резервные копии DE?»: жаргон, и
/// молчание о настройках (решение Wehrwolfmann 15.09.2026 — «сохранить»
/// оставляет и их, и установленные обои и темы; кэш уходит всегда).
pub fn uninstall_keep_hint() -> String {
    // TRANSLATORS: %1 is the "Uninstall but Keep Settings and Backups" button.
    trf(
        "“%1” keeps your settings, backups, and installed wallpapers, themes \
         and add-ons: if you install Rexon again later, it picks up where you \
         left off.",
        &[&uninstall_keep_label()],
    )
}

/// Заголовок подробностей: где лежат данные человека.
pub fn uninstall_data_paths_title() -> String {
    tr("Where it is stored:")
}
/// Пометка категории в подробностях: удаляется при любом выборе.
pub fn uninstall_data_always_mark() -> String {
    tr("removed with either choice")
}
/// Пометка категории в подробностях: только при «Удалить всё».
pub fn uninstall_data_full_mark() -> String {
    // TRANSLATORS: %1 is the "Remove Everything" button.
    trf("only with “%1”", &[&uninstall_delete_label()])
}

/// Подробности: пути данных человека по категориям, в порядке
/// [`UserDataKind::ALL`]. Пустых категорий нет.
pub fn uninstall_data_paths_text(data: &[(UserDataKind, String)]) -> String {
    let mut lines = Vec::new();
    for kind in UserDataKind::ALL {
        let paths: Vec<&String> = data.iter().filter(|(k, _)| *k == kind).map(|(_, p)| p).collect();
        if paths.is_empty() {
            continue;
        }
        let mark = if kind.removed_when_keeping() {
            uninstall_data_always_mark()
        } else {
            uninstall_data_full_mark()
        };
        lines.push(format!("{} — {mark}", kind.title()));
        lines.extend(paths.into_iter().map(|p| format!("  • {p}")));
    }
    lines.join("\n")
}

/// Пояснение под первой строкой: перечень программы, что удаляет каждый
/// выбор словами для человека, и пути данных — подробностями в конце.
pub fn uninstall_informative(files: &[String], data: &[(UserDataKind, String)]) -> String {
    // Внутри <pre> перевод строки остаётся переводом строки: общий
    // экранировщик превращает его в <br>, а буквальный «<br>» из пути он
    // уже превратил в &lt;br&gt;, так что обратная замена трогает только наши.
    let esc = |s: &str| crate::ui_base::escape_dialog_text(s).replace("<br>", "\n");
    // Блоки путей — переносимые (`dialog_pre`): длинный путь в голом <pre>
    // уходил за правый край окна, и хвост того, что удалится, был не виден.
    let pre = |s: &str| crate::ui_base::dialog_pre(&esc(s));
    let mut out = format!(
        "{}<p>{}</p><p>{}</p><p>{}</p><p>{}</p>",
        pre(&uninstall_files_text(files)),
        uninstall_always_line(),
        uninstall_delete_all_line(),
        uninstall_delete_all_active_line(),
        uninstall_keep_hint(),
    );
    if !data.is_empty() {
        out.push_str(&format!(
            "<p>{}</p>{}",
            uninstall_data_paths_title(),
            pre(&uninstall_data_paths_text(data))
        ));
    }
    out
}

/// «Удалить программу, а настройки и резервные копии рабочих столов
/// оставить» — кнопка по смыслу безопасная. Подпись длинная намеренно:
/// человек должен видеть на самой кнопке, что сохранится (прежде —
/// «Удалить (сохранить бэкапы)», и настройки при этом сносились).
pub fn uninstall_keep_label() -> String {
    tr("Uninstall but Keep Settings and Backups")
}
/// «Снести всё, включая настройки и резервные копии» — необратимо.
pub fn uninstall_delete_label() -> String {
    tr("Remove Everything")
}
/// Отказ. В продукте он же кнопка по умолчанию: защита от дурака —
/// случайный Enter не должен снести программу.
pub fn uninstall_cancel_label() -> String {
    // TRANSLATORS: the button that closes a dialog without doing anything.
    trc("dialog button", "Cancel")
}

/// Кнопка по умолчанию окна удаления — «Отмена».
pub const UNINSTALL_DEFAULT_ID: &str = "cancel";

/// Заголовок сообщения об успешном удалении.
pub fn uninstall_done_title() -> String {
    tr("Uninstall Complete")
}

/// Текст сообщения об успешном удалении: итог ядра плюс предупреждение о
/// закрытии окна.
pub fn uninstall_done_text(message: &str) -> String {
    // TRANSLATORS: %1 is the uninstall summary.
    trf("%1\n\nRexon will now close.", &[&message])
}

/// Заголовок сообщения о неудаче удаления.
pub fn uninstall_failed_title() -> String {
    tr("Uninstall Failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Сверки модуля — на исходном языке: тексты идут через каталог, а язык
    /// — состояние всего процесса. Русский «как было» сверяет
    /// `tests/window_menu_parity.rs` на настоящем `po/ru.po`.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    #[test]
    fn menu_has_four_items_in_product_order_with_one_separator() {
        english();
        let ids: Vec<&str> = menu_items().iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![ID_RECOVER_KDE, ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        let seps: Vec<&str> = menu_items()
            .iter()
            .filter(|i| i.separator_before)
            .map(|i| i.id)
            .collect();
        assert_eq!(seps, vec![ID_QUIT], "разделитель стоит только перед «Выходом»");
    }

    #[test]
    fn recover_item_only_in_plasma_session() {
        english();
        let ids = |d: &str| -> Vec<&str> { menu_items_for(d).iter().map(|i| i.id).collect() };
        assert_eq!(ids("KDE"), vec![ID_RECOVER_KDE, ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        assert_eq!(ids("MATE"), vec![ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        assert_eq!(ids("ubuntu:GNOME"), vec![ID_ABOUT, ID_UNINSTALL, ID_QUIT]);
        let seps: Vec<&str> = menu_items_for("MATE")
            .iter()
            .filter(|i| i.separator_before)
            .map(|i| i.id)
            .collect();
        assert_eq!(seps, vec![ID_QUIT]);
    }

    /// Подпись «Выхода» обязана приходить из лотка, а не из копии здесь.
    #[test]
    fn quit_label_comes_from_tray() {
        english();
        let quit = menu_items().into_iter().find(|i| i.id == ID_QUIT).unwrap();
        assert_eq!(quit.label, tray::titled("quit"));
    }

    #[test]
    fn only_quit_has_a_shortcut() {
        english();
        let with: Vec<&str> = menu_items()
            .iter()
            .filter(|i| !i.shortcut.is_empty())
            .map(|i| i.id)
            .collect();
        assert_eq!(with, vec![ID_QUIT]);
    }

    #[test]
    fn unknown_menu_id_is_not_available() {
        english();
        assert!(!is_enabled("нет-такого-пункта"));
        for item in menu_items() {
            assert!(is_enabled(item.id), "пункт «{}» обязан быть доступен", item.id);
        }
    }

    #[test]
    fn only_uninstall_is_destructive() {
        english();
        let danger: Vec<&str> = menu_items()
            .iter()
            .filter(|i| is_destructive(i.id))
            .map(|i| i.id)
            .collect();
        assert_eq!(danger, vec![ID_UNINSTALL]);
        assert!(!is_destructive("нет-такого-пункта"));
    }

    #[test]
    fn recover_calls_own_window_binary_not_python_entry() {
        english();
        let flag = crate::app::RECOVER_KDE_FLAG;
        assert_eq!(
            recover_argv(Some("/usr/lib/rexon/rexon-main-window")),
            vec!["/usr/lib/rexon/rexon-main-window", flag]
        );
        // После пересборки ядро Linux дописывает хвост — звать надо файл,
        // который лежит по этому пути сейчас.
        assert_eq!(
            recover_argv(Some("/t/rust/target/release/rexon-main-window (deleted)")),
            vec!["/t/rust/target/release/rexon-main-window", flag]
        );
        // Окно в чужом процессе (python3) — только общий вход продукта.
        assert_eq!(recover_argv(Some("/usr/bin/python3.14")), vec!["rexon", flag]);
        assert_eq!(recover_argv(None), vec!["rexon", flag]);
        // Флаг один с разбором маршрута: иначе пункт меню звал бы маршрут,
        // которого бинарь не знает.
        assert_eq!(
            crate::app::route(&[flag.to_string()]),
            crate::app::Route::RecoverKde
        );
    }

    #[test]
    fn recover_refuses_outside_plasma_and_says_why() {
        english();
        assert!(recover_allowed("KDE"));
        assert!(recover_allowed(""));
        assert!(!recover_allowed("GNOME"));
        let t = recover_not_plasma_text("GNOME");
        assert!(t.contains("GNOME") && t.contains("Nothing was changed"), "{t}");
    }

    #[test]
    fn forks_texts_name_packages_escaped_and_warn_about_logout() {
        english();
        let t = forks_text(&["aeroshell-libplasma-git".into(), "a<b".into()]);
        assert!(t.contains("<code>aeroshell-libplasma-git</code>"), "{t}");
        assert!(t.contains("a&lt;b") && !t.contains("a<b"), "{t}");
        assert!(t.contains("resetting the theme will not remove them"), "{t}");
        let c = forks_confirm_text("sudo pacman -Rns x && sudo pacman -S y");
        assert!(c.contains("&amp;&amp;") && !c.contains(" && "), "{c}");
        assert!(c.contains("log out and log back in"), "{c}");
        // Жаргон, который покупателю ничего не говорит, в свои слова окна
        // не попадает (команда в <code> — не слова окна, а то, что выполнится).
        for own in [forks_title(), forks_remove_label(), forks_blocked_title(),
                    forks_confirm_title(), forks_done_text(), forks_running_text()] {
            for jargon in ["fork", "stock", "pacman", "logout"] {
                assert!(!own.to_lowercase().contains(jargon), "«{jargon}» в «{own}»");
            }
        }
        assert_eq!(forks_finished(true, "лог").0, forks_done_title());
        assert_eq!(
            forks_finished(false, "объяснение ядра"),
            (forks_failed_title(), "объяснение ядра".to_string())
        );
    }

    #[test]
    fn forks_steps_are_decided_here_for_both_windows() {
        english();
        assert_eq!(forks_prompt(&[]), None, "замен нет — окна нет");
        let p = forks_prompt(&["aeroshell-libplasma-git".into()]).unwrap();
        let ids: Vec<_> = p.buttons.iter().map(|b| b.id).collect();
        assert_eq!(ids, [FORKS_BUTTON_REMOVE, FORKS_BUTTON_THEME, FORKS_BUTTON_CANCEL]);
        let defaults: Vec<_> = p.buttons.iter().filter(|b| b.default).map(|b| b.id).collect();
        assert_eq!(defaults, [FORKS_BUTTON_REMOVE]);
        assert_eq!(forks_next(FORKS_BUTTON_REMOVE), ForksNext::ConfirmRemoval);
        assert_eq!(forks_next(FORKS_BUTTON_THEME), ForksNext::ThemeQuestion);
        assert_eq!(forks_next(FORKS_BUTTON_CANCEL), ForksNext::Stop);
        // Esc / крестик: снимать системные пакеты без прямого «да» нельзя.
        assert_eq!(forks_next(""), ForksNext::Stop);
        assert_eq!(
            serde_json::to_value(ForksNext::ThemeQuestion).unwrap(),
            ForksNext::ThemeQuestion.as_str()
        );
        let ask = forks_preview_step(true, "a && b");
        assert!(ask.ok && ask.title == forks_confirm_title() && ask.text.contains("&amp;&amp;"));
        let no = forks_preview_step(false, "нечем вернуть");
        assert!(!no.ok && no.title == forks_blocked_title() && no.text == "нечем вернуть");
        assert!(no.running_text.is_empty());
    }

    #[test]
    fn recover_target_falls_back_when_backup_is_empty() {
        english();
        assert_eq!(recover_target(None), RECOVER_FALLBACK_TARGET);
        assert_eq!(recover_target(Some("   ")), RECOVER_FALLBACK_TARGET);
        assert_eq!(recover_target(Some(" Breeze \n")), "Breeze");
    }

    /// Остаток обязан быть посчитан, а не отброшен: молча урезанный список
    /// соврал бы о том, что удаляется.
    #[test]
    fn long_file_list_is_cut_and_the_rest_is_counted() {
        english();
        let files: Vec<String> = (0..20).map(|i| format!("/tmp/rexon-{i}")).collect();
        let text = uninstall_files_text(&files);
        assert_eq!(text.lines().count(), UNINSTALL_FILES_SHOWN + 1);
        assert!(text.ends_with("  • … 5 more"), "{text}");
        // Короткий список счёта остатка не получает.
        let short = uninstall_files_text(&files[..3]);
        assert!(!short.contains("more"), "{short}");
    }

    /// Решение 15.09.2026: окно говорит словами, что удаляет каждый выбор, а
    /// пути данных — подробностями, с пометкой, при каком выборе они уходят.
    #[test]
    fn uninstall_prompt_says_what_each_choice_removes() {
        english();
        use UserDataKind::*;
        let data = vec![
            (Cache, "/h/.cache/rexon".to_string()),
            (Settings, "/h/.config/rexon".into()),
            (Backups, "/h/.local/share/rexon/backups".into()),
            (Content, "/h/.local/share/rexon/wallpapers".into()),
            (Content, "/h/.local/share/rexon/a&b".into()),
        ];
        let text = uninstall_informative(&["/h/.local/lib/rexon".to_string()], &data);
        for must in [
            uninstall_always_line(),
            uninstall_delete_all_line(),
            uninstall_delete_all_active_line(),
            uninstall_keep_hint(),
            uninstall_data_paths_title(),
        ] {
            assert!(text.contains(&must), "нет «{must}»: {text}");
        }
        // Строка «сохранить» называет кнопку дословно — человек узнаёт её.
        assert!(uninstall_keep_hint().contains(&uninstall_keep_label()));
        assert!(uninstall_delete_all_line().contains(&uninstall_delete_label()));
        // Слова для человека — без путей и без «кэш».
        // Предупреждение называет кнопку дословно и говорит, что будет со столом.
        assert!(uninstall_delete_all_active_line().contains(&uninstall_delete_label()));
        assert!(uninstall_delete_all_active_line().contains("login screen"));
        for words in [
            uninstall_always_line(),
            uninstall_delete_all_line(),
            uninstall_delete_all_active_line(),
            uninstall_keep_hint(),
        ] {
            assert!(!words.contains('/') && !words.to_lowercase().contains("cache"), "{words}");
        }
        for w in ["temporary files", "wallpapers", "themes", "settings", "backups"] {
            let all = format!("{} {}", uninstall_always_line(), uninstall_delete_all_line()).to_lowercase();
            assert!(all.contains(w), "окно молчит про «{w}»");
        }
        let paths = uninstall_data_paths_text(&data);
        assert_eq!(
            paths,
            "Rexon's temporary files — removed with either choice\n  • /h/.cache/rexon\n\
             Rexon settings — only with “Remove Everything”\n  • /h/.config/rexon\n\
             Desktop backups — only with “Remove Everything”\n  • /h/.local/share/rexon/backups\n\
             Installed wallpapers, themes and add-ons — only with “Remove Everything”\n  • /h/.local/share/rexon/wallpapers\n  • /h/.local/share/rexon/a&b"
        );
        assert!(text.contains("a&amp;b") && !text.contains("a&b"), "пути экранированы: {text}");
        // Данных нет — подробностей нет.
        assert!(!uninstall_informative(&[], &[]).contains(&uninstall_data_paths_title()));
    }

    #[test]
    fn about_names_the_same_version_as_the_crate_constant() {
        english();
        assert!(about_html().contains(&format!("<h2>Rexon {}</h2>", version_string())));
    }
}
