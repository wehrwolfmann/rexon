//! Значок Rexon в системном лотке: состав меню, подписи, подсказка, иконка —
//! и автозапуск при входе (Rexon поднимается сразу в лоток).
//!
//! ГРАНИЦА. Здесь только решения и тексты: какой пункт с каким номером, как он
//! подписан на языке покупателя, включён ли он, отмечен ли, куда смотреть за
//! иконкой, что написать в записи автозапуска. Ни одной строки Qt: сам
//! `QSystemTrayIcon`, `QMenu` и запись файла — на исполнителе
//! (`rexon-gui/src/tray.rs` поверх шима `cpp/rexon_tray.cpp`). Поэтому весь
//! модуль проверяется обычными тестами ядра, без живого лотка.
//!
//! ⚠️ ПОДПИСИ ЖИВУТ ТОЛЬКО ЗДЕСЬ. Ни в мосте, ни в QML, ни в шиме человеческого
//! текста быть не должно — то же правило, по которому устроены вкладки
//! («Настройки» берут каталог `qt_settings::TEXTS`, «Сравнение DE» — плоские
//! константы `qt_compare`).
//!
//! ЗАЧЕМ ЛОТОК ВООБЩЕ. Движок обоев (waywallen) Rexon запускает с `--no-tray`
//! (`we_client::DAEMON_ARGS`), то есть СВОЙ значок движок намеренно не
//! показывает: у продукта один значок, и это значок Rexon. Плата за это —
//! покупатель теряет всё, что давал лоток движка (пауза, следующие обои,
//! звук, интервал смены). Поэтому меню ниже повторяет состав лотка движка и
//! добавляет своё: окно Rexon, переход в раздел и автозапуск.

use crate::i18n::{n_, tr};

// ============================================================================
// Номера пунктов
// ============================================================================
//
// Номер — единственное, что ходит между Qt-слоем и этим модулем: шим
// возвращает нажатый номер, ядро решает, что он значит. Значения стабильны и
// разрежены по группам, чтобы новый пункт не сдвигал соседей.

/// Открыть (поднять) главное окно Rexon.
pub const ID_OPEN: i32 = 1;
/// Начало полосы «Разделы»: `ID_SECTION_BASE + порядковый номер вкладки`.
pub const ID_SECTION_BASE: i32 = 10;
/// Следующие обои.
pub const ID_NEXT: i32 = 20;
/// Предыдущие обои.
pub const ID_PREV: i32 = 21;
/// Перемешивание очереди (флажок).
pub const ID_SHUFFLE: i32 = 30;
/// Начало полосы интервалов смены: `ID_ROTATE_BASE + номер в [`ROTATE_PRESETS`]`.
pub const ID_ROTATE_BASE: i32 = 40;
/// Пауза/возобновление обоев.
pub const ID_PAUSE: i32 = 50;
/// Звук обоев.
pub const ID_MUTE: i32 = 51;
/// Пересканировать библиотеку обоев.
pub const ID_RESCAN: i32 = 60;
/// Запускать Rexon при входе в систему (флажок).
pub const ID_AUTOSTART: i32 = 70;
/// Строка «Rexon может изменять систему…» (не нажимается, только видна,
/// пока есть сеанс прав).
pub const ID_ADMIN_INFO: i32 = 80;
/// «Запретить сейчас»: закрыть сеанс прав (следующее изменение спросит
/// пароль снова).
pub const ID_REVOKE: i32 = 81;
/// Выход из Rexon.
pub const ID_QUIT: i32 = 99;

// ============================================================================
// Подписи
// ============================================================================

/// Подписи меню: ключ → английский `msgid` каталога перевода.
///
/// Таблица плоская и КЛЮЧЕВАЯ: ключ виден в коде исполнителя, а текст
/// переводится в месте показа ([`label`] → `i18n::tr`). Русский и любой
/// другой язык живут в `po/<код>.po`, второй копии текста здесь нет.
const LABELS: &[(&str, &str)] = &[
    ("open", n_("Open Rexon")),
    // TRANSLATORS: submenu of the tray icon listing the tabs of the main window.
    ("sections", n_("Sections")),
    ("next", n_("Next Wallpaper")),
    ("prev", n_("Previous Wallpaper")),
    ("shuffle", n_("Shuffle")),
    // TRANSLATORS: submenu of the tray icon with the wallpaper rotation intervals.
    ("rotate", n_("Change Wallpaper Every")),
    ("pause", n_("Pause")),
    ("resume", n_("Resume")),
    ("mute", n_("Mute")),
    ("unmute", n_("Unmute")),
    ("rescan", n_("Rescan Wallpapers")),
    // Одна подпись с флажком в «Настройках» (`qt_settings`, ключ
    // `tray.autostart`): это одна и та же настройка.
    ("autostart", n_("Start at login")),
    ("quit", n_("Quit")),
    // Сеанс прав (Б13): одна подпись с полосой в «Настройках»
    // (`qt_settings`, ключи `admin.*`) — это одно и то же состояние.
    // TRANSLATORS: shown while Rexon may change the system without asking for the password again.
    ("admin.info", n_("Rexon can change the system until the window is closed")),
    // TRANSLATORS: button/menu item: stop letting Rexon change the system; next time it asks for the password.
    ("admin.revoke", n_("Revoke Now")),
    // Интервалы смены обоев.
    // TRANSLATORS: wallpaper rotation interval: never change the wallpaper.
    ("rot.off", n_("Off")),
    ("rot.30s", n_("30 Seconds")),
    ("rot.1m", n_("1 Minute")),
    ("rot.5m", n_("5 Minutes")),
    ("rot.15m", n_("15 Minutes")),
    ("rot.1h", n_("1 Hour")),
    // Разделы главного окна — идентификаторы те же, что у таблицы вкладок
    // главного окна (`crate::qt_main::SECTIONS`).
    ("tab.desktops", n_("Desktops")),
    ("tab.appearance", n_("Appearance")),
    ("tab.extensions", n_("Extensions")),
    // Имена продуктов не переводятся — `tr` вернёт их как есть.
    ("tab.wallpaper_engine", "Wallpaper Engine"),
    ("tab.steam", "Steam"),
    ("tab.settings", n_("Settings")),
    // Подсказка значка и уведомление о сворачивании.
    ("tip.title", "Rexon"),
    // TRANSLATORS: tooltip of the tray icon, under the name "Rexon".
    ("tip.body", n_("Desktop environment manager")),
    ("hidden.title", n_("Rexon is still running in the tray")),
    (
        "hidden.body",
        n_("The window is closed, but Rexon keeps running. \
            Click the icon to open it again, or choose “Quit” in the icon menu to exit."),
    ),
];

/// `msgid` подписи по ключу — для сверок, которым нужен исходный текст, а не
/// перевод на язык процесса.
pub fn label_msgid(key: &str) -> Option<&'static str> {
    LABELS.iter().find(|(k, _)| *k == key).map(|(_, m)| *m)
}

/// Подпись по ключу на языке интерфейса. Неизвестный ключ — пустая строка:
/// молча промолчать здесь безопаснее, чем показать ключ вместо текста.
pub fn label(key: &str) -> String {
    label_msgid(key).map(tr).unwrap_or_default()
}

/// Подпись раздела главного окна по его идентификатору (`desktops`,
/// `appearance`, …). Незнакомый идентификатор отдаёт его самого — так новая
/// вкладка окажется в меню подписанной хоть как-то, а не пустой строкой.
pub fn section_label(tab_id: &str) -> String {
    let text = label(&format!("tab.{tab_id}"));
    if text.is_empty() {
        tab_id.to_string()
    } else {
        text
    }
}

// ============================================================================
// Значки подписей
// ============================================================================

/// Значок подписи: ключ таблицы [`LABELS`] → символ, которым эта же подпись
/// выходит в ГЛАВНОМ ОКНЕ.
///
/// ЗАЧЕМ ОТДЕЛЬНОЙ ТАБЛИЦЕЙ, А НЕ В САМОЙ ПОДПИСИ. Один и тот же раздел
/// покупатель видит дважды и в двух видах: в полосе вкладок окна со значком
/// («🖥 Рабочие столы») и в подменю «Разделы» значка в лотке без него
/// («Рабочие столы» — значки на части пунктов меню и не на остальных
/// читались бы как рябь). Текст при этом ОДИН, и второй его копии заводить
/// нельзя: подписи живут только в [`LABELS`], здесь — только украшение.
///
/// ⚠️ ЧТО БЫЛО ДО 07.09.2026. Значки лежали в Python — в таблице `_TABS`
/// (`rexon/qt_main/window.py`), вместе с подписями, которых у окна не должно
/// быть вовсе. Ядро не умело выдать то, что видит покупатель, а окно
/// вдобавок не переводилось, хотя лоток переводится: сверка каркаса
/// (`tests/qt_main_parity.rs`) на этом и падала. Новый значок — строка
/// здесь, а не литерал в окне.
///
/// ⚠️ U+FE0F после «🖥» и «🖼» не мусор. У этих двух символов (U+1F5A5,
/// U+1F5BC) вид по умолчанию текстовый: без U+FE0F подбор шрифта отдаёт их
/// одноцветному шрифту символов, и в полосе вкладок они стояли чёрно-белыми
/// рядом с цветными соседями. Новый значок проверять тем же: если у символа
/// вид по умолчанию текстовый, U+FE0F к нему обязателен.
const ICONS: &[(&str, &str)] = &[
    ("tab.desktops", "🖥️"),
    ("tab.appearance", "🎨"),
    ("tab.extensions", "🧩"),
    ("tab.wallpaper_engine", "🖼️"),
    ("tab.steam", "🎮"),
    ("tab.settings", "⚙️"),
    ("quit", "🚪"),
    ("admin.info", "🔓"),
];

/// Значок подписи; у ключа без значка — пустая строка.
pub fn icon(key: &str) -> &'static str {
    ICONS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, glyph)| *glyph)
        .unwrap_or("")
}

/// Подпись со значком на языке покупателя — ровно та строка, которую
/// показывает главное окно («🖥 Рабочие столы», «🚪 Выход»).
///
/// Без значка отдаёт голую подпись, то есть годится для любого ключа.
pub fn titled(key: &str) -> String {
    let text = label(key);
    let glyph = icon(key);
    if glyph.is_empty() || text.is_empty() {
        text
    } else {
        format!("{glyph} {text}")
    }
}

// ============================================================================
// Интервалы смены обоев
// ============================================================================

/// Пресеты подменю «Смена обоев»: ключ подписи и интервал в секундах.
///
/// Одна таблица на всё: и подписи, и радио-отметку, и то, что уходит демону.
/// Новый пресет — одна строка здесь, больше нигде.
pub const ROTATE_PRESETS: &[(&str, i64)] = &[
    ("rot.off", 0),
    ("rot.30s", 30),
    ("rot.1m", 60),
    ("rot.5m", 300),
    ("rot.15m", 900),
    ("rot.1h", 3600),
];

/// Секунды пресета по номеру пункта; `None` — номер не из полосы интервалов.
pub fn rotate_secs_for(id: i32) -> Option<i64> {
    let index = usize::try_from(id - ID_ROTATE_BASE).ok()?;
    ROTATE_PRESETS.get(index).map(|(_, secs)| *secs)
}

// ============================================================================
// Состояние, по которому строится меню
// ============================================================================

/// Снимок живого состояния на момент открытия меню.
///
/// ⚠️ `paused` и `muted` — НАШ учёт, а не ответ демона: интерфейс демона на
/// шине даёт методы `Pause`/`Resume`/`Mute`/`Unmute`, но свойств «сейчас на
/// паузе» и «сейчас заглушено» у него нет вовсе (проверено интроспекцией
/// `org.waywallen.waywallen.Daemon1` версии 0.3.7 — там же, где есть
/// `QueueMode` и `RotationSecs`). Исполнитель обязан сбрасывать оба флага,
/// когда демон пропадал с шины: перезапущенный демон играет и звучит.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrayState {
    /// Демон обоев виден на шине — иначе вся полоса обоев погашена.
    pub daemon: bool,
    /// Очередь перемешана (`QueueMode == "shuffle"`).
    pub shuffle: bool,
    /// Интервал автосмены в секундах (`RotationSecs`).
    pub rotation_secs: i64,
    /// Обои на паузе (наш учёт, см. предупреждение выше).
    pub paused: bool,
    /// Звук обоев заглушён (наш учёт).
    pub muted: bool,
    /// Включён автозапуск Rexon при входе.
    pub autostart: bool,
    /// Есть сеанс прав окна (`privilege::run::has_session`): пароль уже
    /// введён, и Rexon может изменять систему до закрытия окна.
    pub admin_session: bool,
}

/// Что за пункт меню — исполнителю хватает этого, чтобы построить `QMenu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// Обычное действие.
    Action,
    /// Разделитель (номер и подпись не значат ничего).
    Separator,
    /// Флажок.
    Check,
    /// Кружок в группе взаимоисключающих.
    Radio,
    /// Начало вложенного меню; до парного [`ItemKind::SubmenuEnd`] пункты
    /// уходят внутрь него.
    SubmenuBegin,
    /// Конец вложенного меню.
    SubmenuEnd,
}

/// Готовая строка меню.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: i32,
    pub kind: ItemKind,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
    /// Пункт по умолчанию (жирный) — тот, что делает щелчок по значку.
    pub default: bool,
}

impl MenuItem {
    fn new(id: i32, kind: ItemKind, label: String) -> Self {
        MenuItem {
            id,
            kind,
            label,
            checked: false,
            enabled: true,
            default: false,
        }
    }

    fn sep() -> Self {
        MenuItem::new(0, ItemKind::Separator, String::new())
    }

    fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }

    fn default(mut self) -> Self {
        self.default = true;
        self
    }
}

/// Состав меню значка.
///
/// `tab_ids` — идентификаторы вкладок главного окна В ТОМ ЖЕ ПОРЯДКЕ, в каком
/// они стоят в окне: их передаёт сам каркас окна, поэтому подменю «Разделы»
/// не может разъехаться с окном, даже если вкладку добавят или переставят.
/// Пустой список — подменю не строится вовсе (окна с вкладками нет).
///
/// Порядок групп — как у лотка движка обоев: сперва глаголы, потом
/// переключатели состояния, потом обслуживание, выход последним.
pub fn menu(tab_ids: &[String], state: TrayState) -> Vec<MenuItem> {
    let t = |key: &str| label(key);
    let mut out = vec![MenuItem::new(ID_OPEN, ItemKind::Action, t("open")).default()];

    if !tab_ids.is_empty() {
        out.push(MenuItem::new(0, ItemKind::SubmenuBegin, t("sections")));
        for (index, id) in tab_ids.iter().enumerate() {
            let number = ID_SECTION_BASE + index as i32;
            out.push(MenuItem::new(
                number,
                ItemKind::Action,
                section_label(id),
            ));
        }
        out.push(MenuItem::new(0, ItemKind::SubmenuEnd, String::new()));
    }

    // ── Право изменять систему (Б13): видно, только пока оно есть, — и
    // тут же способ его снять, не закрывая окно.
    if state.admin_session {
        out.push(MenuItem::sep());
        out.push(MenuItem::new(ID_ADMIN_INFO, ItemKind::Action, titled("admin.info")).enabled(false));
        out.push(MenuItem::new(ID_REVOKE, ItemKind::Action, t("admin.revoke")));
    }

    // ── Обои. Вся полоса гаснет без демона: жать «Следующие обои», когда
    // играть нечему, покупателю нечего (правило #13 — защита от дурака).
    let live = state.daemon;
    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_NEXT, ItemKind::Action, t("next")).enabled(live));
    out.push(MenuItem::new(ID_PREV, ItemKind::Action, t("prev")).enabled(live));
    out.push(
        MenuItem::new(ID_SHUFFLE, ItemKind::Check, t("shuffle"))
            .checked(state.shuffle)
            .enabled(live),
    );

    out.push(MenuItem::new(0, ItemKind::SubmenuBegin, t("rotate")).enabled(live));
    for (index, (key, secs)) in ROTATE_PRESETS.iter().enumerate() {
        out.push(
            MenuItem::new(ID_ROTATE_BASE + index as i32, ItemKind::Radio, t(key))
                .checked(state.rotation_secs == *secs)
                .enabled(live),
        );
    }
    out.push(MenuItem::new(0, ItemKind::SubmenuEnd, String::new()));

    out.push(MenuItem::sep());
    // Глагол, а не флажок: флажок с подписью «Пауза» не отвечает на вопрос
    // «отмечено — это сейчас пауза или сейчас играет».
    out.push(
        MenuItem::new(
            ID_PAUSE,
            ItemKind::Action,
            t(if state.paused { "resume" } else { "pause" }),
        )
        .enabled(live),
    );
    out.push(
        MenuItem::new(
            ID_MUTE,
            ItemKind::Action,
            t(if state.muted { "unmute" } else { "mute" }),
        )
        .enabled(live),
    );

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_RESCAN, ItemKind::Action, t("rescan")).enabled(live));

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_AUTOSTART, ItemKind::Check, t("autostart")).checked(state.autostart));

    out.push(MenuItem::sep());
    out.push(MenuItem::new(ID_QUIT, ItemKind::Action, t("quit")));
    out
}

// ============================================================================
// Значок
// ============================================================================

/// Имя иконки в теме — то же, что стоит в `Icon=` ярлыка приложения
/// (`packaging/aur/rexon.desktop`) и под которым пакет кладёт файлы в
/// `hicolor`. Оболочка ищет его сама и сама берёт нужный размер.
pub const ICON_THEME_NAME: &str = "rexon";

/// Чем Rexon представляется системе — `QCoreApplication::applicationName`
/// у точки входа главного окна.
///
/// ⭐ ЗАЧЕМ ЭТО ЗДЕСЬ, В ЛОТКЕ. У Qt это же имя УХОДИТ НА ШИНУ свойством
/// `Id` значка StatusNotifierItem, и другого рычага нет: `Id` и `Title`
/// читаются обоюдно из `QCoreApplication::applicationName()`
/// (`QStatusNotifierItemAdaptor::id`/`::title`, qtbase). А оболочка помнит
/// по `Id` пользовательские настройки самого значка — показывать его в
/// лотке или прятать и в каком порядке (в Plasma это списки `shownItems` и
/// `hiddenItems` апплета лотка).
///
/// Поэтому имя обязано быть ПОСТОЯННЫМ и не зависеть от языка. Сюда попадал
/// переведённый заголовок окна (`qt_main::WINDOW_TITLE`): покупатель,
/// сменивший язык, получал значок с новым `Id` — то есть терял свои же
/// настройки и видел «новый», невесть откуда взявшийся значок.
///
/// ⚠️ Строка видна и человеку: ту же `Title` оболочка печатает в списке
/// значков («Записи» в настройках лотка) и во всплывающем списке скрытых.
/// Отсюда «Rexon», а не `rexon`: техническим токеном покупателю в глаза
/// светить нельзя (правило #13), а имя продукта одинаково хорошо годится и
/// машине, и человеку. Человеческое описание живёт в подсказке значка
/// (`tooltip_title` + `tooltip_body`) — его перевод ничему не мешает.
///
/// Значение — то же, что у питоновского хозяина того же окна в общей
/// таблице точек входа (`qt_app::ENTRY_IDENTITIES`, ключ `rexon.qt_main.app`);
/// сторож — тест `имя_на_шине_машинное_и_совпадает_с_таблицей_точек_входа`.
pub const APP_ID: &str = "Rexon";

/// Где искать файл значка, если темы не хватило, — по убыванию свежести.
///
/// Тот же приём и та же причина, что у списка каталогов наших `.so`
/// (`qml_modules::rust_lib_candidates`): у покупателя дерева репозитория
/// нет, а у разработчика нет установленного пакета. Пустой значок в лотке
/// не показывается ВООБЩЕ —
/// значок без картинки покупатель считает пропавшим приложением.
pub fn icon_candidates(repo_root: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !repo_root.is_empty() {
        // Разработческое дерево: мастер-копия и крупные размеры лежат в корне.
        out.push(format!("{repo_root}/rexon_icon_256.png"));
        out.push(format!("{repo_root}/rexon_icon_128.png"));
        out.push(format!("{repo_root}/rexon_icon.png"));
    }
    out.push("/usr/lib/rexon/rexon_icon.png".to_string());
    out.push("/usr/local/lib/rexon/rexon_icon.png".to_string());
    out.push("/usr/share/icons/hicolor/256x256/apps/rexon.png".to_string());
    out.push("/usr/share/pixmaps/rexon.png".to_string());
    out
}

/// Корень дерева разработчика по пути своего же исполняемого файла.
///
/// Нужен только значку и только за одним — найти файл картинки, когда темы
/// значков не хватило (`icon_candidates`). У продукта на Python корень
/// считается от `__file__`, а у бинаря на Rust `__file__` нет: он лежит в
/// `<корень>/rust/target/{debug,release}/rexon-main-window`, поэтому корень —
/// три каталога вверх, и только если по пути действительно `rust/target`.
///
/// Пустой ответ — не ошибка, а честное «дерева нет»: у покупателя бинарь
/// стоит в `/usr/bin`, и файл значка ему найдут системные пути того же
/// списка. Гадать «а вдруг это дерево» нельзя: тогда список кандидатов
/// начинался бы с трёх заведомо мёртвых путей у каждого покупателя.
pub fn repo_root_from_exe(exe: &str) -> String {
    let path = std::path::Path::new(exe);
    // <корень>/rust/target/<профиль>/<бинарь>
    let profile = match path.parent() {
        Some(p) => p,
        None => return String::new(),
    };
    let target = match profile.parent() {
        Some(p) if p.file_name().is_some_and(|n| n == "target") => p,
        _ => return String::new(),
    };
    let rust = match target.parent() {
        Some(p) if p.file_name().is_some_and(|n| n == "rust") => p,
        _ => return String::new(),
    };
    match rust.parent() {
        Some(root) if !root.as_os_str().is_empty() => root.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// Заголовок подсказки значка.
pub fn tooltip_title() -> String {
    label("tip.title")
}

/// Текст подсказки значка.
pub fn tooltip_body() -> String {
    label("tip.body")
}

/// Заголовок и текст уведомления «окно закрылось, Rexon остался в лотке».
///
/// Показывается ОДИН раз за установку: молча спрятать окно — ровно тот
/// «тихо убил приложение», которого быть не должно, а показывать это каждый
/// раз — навязчиво.
pub fn hidden_notice() -> (String, String) {
    (label("hidden.title"), label("hidden.body"))
}

/// Ключ настройки «уведомление о сворачивании уже показывали».
pub const SEEN_HIDDEN_NOTICE_KEY: &str = "tray_hidden_notice_seen";

// ============================================================================
// Автозапуск при входе
// ============================================================================

/// Флаг «подняться сразу в лоток, окно не показывать».
pub const START_HIDDEN_FLAG: &str = "--tray";

/// Просили ли поднять Rexon сразу в лоток (`args` — без имени программы).
pub fn starts_hidden(args: &[String]) -> bool {
    args.iter().any(|a| a == START_HIDDEN_FLAG)
}

/// Путь записи автозапуска Rexon.
///
/// Механизм тот же, что у заставки входа (`session_splash::autostart_path`) —
/// `.desktop` в каталоге автозапуска рабочего стола. Запись СВОЯ и отдельная:
/// у заставки нарочно другой механизм (systemd-юнит вместо `.desktop`, потому
/// что `.desktop` стартует уже после `plasmashell`), у переключателя дефолтных
/// приложений — третья запись, и мешать их в одну нельзя.
pub fn autostart_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = match xdg_config_home {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => format!("{home}/.config"),
    };
    format!("{cfg}/autostart/rexon.desktop")
}

/// Команда запуска для записи автозапуска.
///
/// ⚠️ Вход выбирает ТА ЖЕ функция, что и у ярлыка
/// ([`crate::installer::launch_entry_live`]), — сюда он приходит готовым в
/// `entry`. Лоток — весь хозяин процесса, замок одного экземпляра общий:
/// автозапуск Python-лотка при ярлыке на Rust-окно превратил бы щелчок по
/// ярлыку в показ питоновского окна (Rust-бинарь постучится в занятый замок
/// и выйдет). Поэтому оба пишутся по одному правилу.
///
/// Rust-вход — `rexon-main-window --tray`. Python-вход (откат) — лаунчер
/// `rexon` полным путём: `rexon_path` — что нашёл `which rexon` (пусто или
/// `None` — не нашёл).
///
/// `None` — заводить нечем; запись автозапуска тогда НЕ пишется
/// ([`crate::app::MSG_NOTHING_TO_LAUNCH`]). ⛔Прежний запасной путь
/// `python3 -m rexon.app --tray` снят 16.09.2026 — почему именно, написано
/// у [`crate::app::launch_program`], куда выбор и переехал целиком.
pub fn autostart_exec(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
) -> Option<String> {
    crate::app::launch_command(entry, rexon_path, &[START_HIDDEN_FLAG])
}

/// Подписи записи автозапуска.
///
/// Их видит не покупатель в окне Rexon, а редактор автозапуска рабочего
/// стола, и язык он выбирает САМ по локали сеанса — поэтому в файл ложатся
/// все языки каталога сразу (`Comment[ru]=`), а не язык интерфейса Rexon
/// ([`crate::system_l10n::desktop_key`]).
// TRANSLATORS: Description of the Rexon login autostart entry (shown in the desktop's autostart settings).
const AUTOSTART_COMMENT: &str = n_("Start Rexon in the system tray at login");
// TRANSLATORS: Menu entry of Rexon: GenericName shown under the name "Rexon".
const AUTOSTART_GENERIC: &str = n_("Desktop Environment Manager");

/// Содержимое записи автозапуска.
pub fn autostart_desktop_content(exec: &str) -> String {
    use crate::system_l10n::desktop_key as key;
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon\n\
         {generic}\
         {comment}\
         Exec={exec}\n\
         Icon={ICON_THEME_NAME}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        generic = key("GenericName", AUTOSTART_GENERIC),
        comment = key("Comment", AUTOSTART_COMMENT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Сверки модуля — на исходном языке: подписи идут через каталог, а
    /// язык — состояние всего процесса. Русский «как было» сверяет
    /// `tests/qt_main_parity.rs` на настоящем `po/ru.po`.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn ids() -> Vec<String> {
        ["desktops", "appearance", "extensions", "wallpaper_engine", "steam", "settings"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn обязательный_минимум_на_месте() {
        english();
        let m = menu(&ids(), TrayState::default());
        assert!(m.iter().any(|i| i.id == ID_OPEN && i.default));
        assert!(m.iter().any(|i| i.id == ID_QUIT));
        // Выход — последний пункт: место, где его ищут.
        assert_eq!(m.last().unwrap().id, ID_QUIT);
    }

    #[test]
    fn подменю_разделов_повторяет_порядок_окна() {
        english();
        let m = menu(&ids(), TrayState::default());
        let inside: Vec<&MenuItem> = m
            .iter()
            .skip_while(|i| i.kind != ItemKind::SubmenuBegin)
            .skip(1)
            .take_while(|i| i.kind != ItemKind::SubmenuEnd)
            .collect();
        assert_eq!(inside.len(), 6);
        assert_eq!(inside[0].id, ID_SECTION_BASE);
        assert_eq!(inside[0].label, "Desktops");
        assert_eq!(inside[5].label, "Settings");
    }

    #[test]
    fn без_вкладок_подменю_разделов_не_строится() {
        english();
        let m = menu(&[], TrayState::default());
        assert!(!m
            .iter()
            .any(|i| i.kind == ItemKind::SubmenuBegin && i.label == "Sections"));
    }

    #[test]
    fn без_демона_полоса_обоев_погашена_а_окно_и_выход_живы() {
        english();
        let m = menu(&ids(), TrayState::default());
        let by = |id: i32| m.iter().find(|i| i.id == id).unwrap();
        for id in [ID_NEXT, ID_PREV, ID_SHUFFLE, ID_PAUSE, ID_MUTE, ID_RESCAN] {
            assert!(!by(id).enabled, "пункт {id} обязан быть погашен без демона");
        }
        assert!(by(ID_OPEN).enabled);
        assert!(by(ID_QUIT).enabled);
        assert!(by(ID_AUTOSTART).enabled);
    }

    #[test]
    fn право_изменять_систему_видно_только_с_сеансом() {
        english();
        let without = menu(&ids(), TrayState::default());
        assert!(!without.iter().any(|i| i.id == ID_ADMIN_INFO || i.id == ID_REVOKE));
        let with = menu(&ids(), TrayState { admin_session: true, ..Default::default() });
        let info = with.iter().find(|i| i.id == ID_ADMIN_INFO).unwrap();
        assert!(!info.enabled, "строка состояния не нажимается");
        assert!(info.label.starts_with("🔓 "), "{}", info.label);
        let revoke = with.iter().find(|i| i.id == ID_REVOKE).unwrap();
        assert!(revoke.enabled);
        assert_eq!(revoke.label, "Revoke Now");
    }

    #[test]
    fn подписи_паузы_и_звука_переворачиваются() {
        english();
        let playing = TrayState { daemon: true, ..Default::default() };
        let paused = TrayState { daemon: true, paused: true, muted: true, ..Default::default() };
        let find = |st, id| {
            menu(&ids(), st)
                .into_iter()
                .find(|i| i.id == id)
                .unwrap()
                .label
        };
        assert_eq!(find(playing, ID_PAUSE), "Pause");
        assert_eq!(find(paused, ID_PAUSE), "Resume");
        assert_eq!(find(playing, ID_MUTE), "Mute");
        assert_eq!(find(paused, ID_MUTE), "Unmute");
    }

    #[test]
    fn отмечен_ровно_один_интервал_смены() {
        english();
        let st = TrayState { daemon: true, rotation_secs: 300, ..Default::default() };
        let m = menu(&ids(), st);
        let marked: Vec<&MenuItem> = m
            .iter()
            .filter(|i| i.kind == ItemKind::Radio && i.checked)
            .collect();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].label, "5 Minutes");
        assert_eq!(rotate_secs_for(marked[0].id), Some(300));
    }

    #[test]
    fn чужой_интервал_демона_не_отмечает_ничего() {
        english();
        let st = TrayState { daemon: true, rotation_secs: 42, ..Default::default() };
        let m = menu(&ids(), st);
        assert!(!m.iter().any(|i| i.kind == ItemKind::Radio && i.checked));
    }

    #[test]
    fn номера_интервалов_разбираются_обратно() {
        english();
        for (index, (_, secs)) in ROTATE_PRESETS.iter().enumerate() {
            assert_eq!(rotate_secs_for(ID_ROTATE_BASE + index as i32), Some(*secs));
        }
        assert_eq!(rotate_secs_for(ID_ROTATE_BASE - 1), None);
        assert_eq!(
            rotate_secs_for(ID_ROTATE_BASE + ROTATE_PRESETS.len() as i32),
            None
        );
        // Полоса интервалов не наезжает на соседние группы.
        assert!(ID_ROTATE_BASE + ROTATE_PRESETS.len() as i32 <= ID_PAUSE);
    }

    #[test]
    fn английское_меню_без_кириллицы() {
        english();
        let st = TrayState { daemon: true, ..Default::default() };
        for item in menu(&ids(), st) {
            assert!(
                !item.label.chars().any(|c| ('а'..='я').contains(&c.to_ascii_lowercase())
                    || c == 'ё'),
                "английская подпись содержит кириллицу: {}",
                item.label
            );
        }
    }

    #[test]
    fn каждый_ключ_подписан() {
        english();
        for (key, msgid) in LABELS {
            assert!(!msgid.is_empty(), "нет подписи для {key}");
        }
        // Ключи уникальны — иначе `label` молча возьмёт первый.
        let mut keys: Vec<&str> = LABELS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "в таблице подписей повторяется ключ");
    }

    /// Значок приписывается СУЩЕСТВУЮЩЕЙ подписи: ключ без подписи — это
    /// опечатка, и значок на пустом месте её бы спрятал.
    #[test]
    fn every_icon_has_a_label() {
        english();
        for (key, glyph) in ICONS {
            assert!(!glyph.is_empty(), "пустой значок у {key}");
            assert!(
                label_msgid(key).is_some(),
                "значок {glyph} висит на ключе {key}, которого нет в подписях"
            );
        }
    }

    /// Подпись со значком — это подпись плюс значок, а не третья строка:
    /// перевод обязан доходить до окна так же, как до лотка.
    #[test]
    fn titled_is_icon_plus_label() {
        english();
        assert_eq!(titled("tab.desktops"), "🖥️ Desktops");
        assert_eq!(titled("quit"), "🚪 Quit");
        // Ключ без значка отдаёт голую подпись, а не пустоту.
        assert_eq!(titled("open"), label("open"));
        assert!(titled("нет такого").is_empty());
    }

    /// Меню значка в лотке значков НЕ показывает: они украшение окна.
    #[test]
    fn tray_menu_stays_without_icons() {
        english();
        let ids: Vec<String> = vec!["desktops".into(), "settings".into()];
        for item in menu(&ids, TrayState::default()) {
            for (_, glyph) in ICONS {
                assert!(
                    !item.label.contains(glyph),
                    "в меню лотка пробрался значок {glyph}: {}",
                    item.label
                );
            }
        }
    }

    /// Подпись автозапуска в меню значка и в разделе «Настроек» — ОДНА
    /// настройка, и текст у неё обязан быть один: покупатель не должен
    /// гадать, одно ли это и то же (правило #12 — один движок, две двери).
    #[test]
    fn подпись_автозапуска_одна_на_меню_и_настройки() {
        english();
        // «Настройки» держат английский `msgid` (русский — в `po/ru.po`),
        // поэтому сверяется исходный текст: английская подпись меню значка.
        assert_eq!(
            label_msgid("autostart"),
            crate::qt_settings::text_msgid("tray.autostart")
        );
    }

    #[test]
    fn незнакомый_раздел_подписан_хоть_как_то() {
        english();
        assert_eq!(section_label("desktops"), "Desktops");
        assert_eq!(section_label("новая"), "новая");
    }

    #[test]
    fn флаг_свёрнутого_запуска() {
        english();
        assert!(starts_hidden(&["--tray".to_string()]));
        assert!(starts_hidden(&["--page=install".into(), "--tray".into()]));
        assert!(!starts_hidden(&[]));
        assert!(!starts_hidden(&["--traylike".to_string()]));
    }

    #[test]
    fn путь_записи_автозапуска() {
        english();
        assert_eq!(
            autostart_path(None, "/home/t"),
            "/home/t/.config/autostart/rexon.desktop"
        );
        assert_eq!(
            autostart_path(Some("/cfg"), "/home/t"),
            "/cfg/autostart/rexon.desktop"
        );
        // Своя запись, не чужая: заставка и дефолтные приложения не задеты.
        let mine = autostart_path(None, "/home/t");
        assert_ne!(mine, crate::session_splash::autostart_path(None, "/home/t"));
        assert!(!mine.ends_with("rexon-appdefaults.desktop"));
    }

    #[test]
    fn запись_автозапуска_поднимает_в_лоток() {
        english();
        use crate::installer::LaunchEntry;
        let py = LaunchEntry::Python;
        let exec = autostart_exec(&py, Some("/usr/local/bin/rexon")).expect("лаунчер нашёлся");
        assert_eq!(exec, "/usr/local/bin/rexon --tray");
        let body = autostart_desktop_content(&exec);
        assert!(body.starts_with("[Desktop Entry]\n"));
        assert!(body.contains("\nExec=/usr/local/bin/rexon --tray\n"));
        assert!(body.contains("\nIcon=rexon\n"));
        assert!(body.contains("\nComment[ru]=Запускать Rexon в системном лотке при входе\n"));
        assert!(body.ends_with('\n'));
        // ⛔Заводить нечем — записи НЕ будет вовсе. Прежде тут выдавалось
        // `python3 -m rexon.app --tray`: строка из автозапуска выполняется
        // без рабочего каталога, пакета `rexon` в `sys.path` системного
        // интерпретатора нет — вход в сеанс молча не делал ничего.
        assert_eq!(autostart_exec(&py, None), None);
        assert_eq!(autostart_exec(&py, Some("")), None);
        assert_eq!(autostart_exec(&py, Some("   ")), None);
        // Rust-вход: лоток поднимает Rust-окно, лаунчер `rexon` не при чём.
        let rust = LaunchEntry::MainWindow("/usr/lib/rexon/rexon-main-window".into());
        assert_eq!(
            autostart_exec(&rust, Some("/usr/local/bin/rexon")).as_deref(),
            Some("/usr/lib/rexon/rexon-main-window --tray")
        );
        assert_eq!(
            autostart_exec(&rust, None).as_deref(),
            Some("/usr/lib/rexon/rexon-main-window --tray")
        );
        // Пробел в пути не разрывает команду на два аргумента.
        let spaced = LaunchEntry::MainWindow("/home/w/мой Rexon/rexon-main-window".into());
        assert_eq!(
            autostart_exec(&spaced, None).as_deref(),
            Some("\"/home/w/мой Rexon/rexon-main-window\" --tray")
        );
        // Ни один вход не пишет в автозапуск интерпретатор.
        for e in [&py, &rust, &spaced] {
            if let Some(line) = autostart_exec(e, Some("/usr/local/bin/rexon")) {
                assert!(!line.contains("python"), "в автозапуске интерпретатор: {line}");
                assert!(!line.contains("-m rexon"), "в автозапуске модуль: {line}");
            }
        }
    }

    /// Имя, которое Qt отдаёт на шину свойством `Id` значка, обязано быть
    /// машинным и ОДНИМ на продукт. Оба свойства сторожат живой дефект:
    /// туда попадал переведённый заголовок окна, и у покупателя, сменившего
    /// язык, слетали настройки значка в лотке (оболочка помнит их по `Id`).
    #[test]
    fn имя_на_шине_машинное_и_совпадает_с_таблицей_точек_входа() {
        english();
        // Не заголовок окна и вообще не переводимая строка.
        assert_ne!(APP_ID, crate::qt_main::window_title());
        assert!(APP_ID.is_ascii(), "имя на шине обязано быть латиницей");
        assert!(!APP_ID.contains(' '), "имя на шине — одно слово");
        // Второго имени у продукта нет: та же запись, что у питоновского
        // хозяина главного окна.
        let entry = crate::qt_app::identity("rexon.qt_main.app")
            .expect("главное окно есть в таблице точек входа");
        assert_eq!(APP_ID, entry.application_name);
    }

    #[test]
    fn кандидаты_значка_покрывают_дерево_и_пакет() {
        english();
        let c = icon_candidates("/repo");
        assert_eq!(c[0], "/repo/rexon_icon_256.png");
        assert!(c.iter().any(|p| p == "/usr/share/pixmaps/rexon.png"));
        // Без дерева репозитория остаются только пакетные пути.
        assert!(icon_candidates("").iter().all(|p| p.starts_with("/usr/")));
    }

    /// Корень дерева разработчика по пути бинаря — и, что важнее, ПУСТО там,
    /// где дерева нет. Соврав здесь, значок начал бы искать картинку в трёх
    /// заведомо мёртвых путях у каждого покупателя.
    #[test]
    fn корень_дерева_считается_по_пути_бинаря() {
        english();
        assert_eq!(
            repo_root_from_exe("/home/u/Rexon/rexon_v5/rust/target/release/rexon-main-window"),
            "/home/u/Rexon/rexon_v5"
        );
        assert_eq!(
            repo_root_from_exe("/home/u/Rexon/rexon_v5/rust/target/debug/rexon-tray-probe"),
            "/home/u/Rexon/rexon_v5"
        );
        // У покупателя дерева нет — и выдумывать его нельзя.
        assert_eq!(repo_root_from_exe("/usr/bin/rexon"), "");
        assert_eq!(repo_root_from_exe("/usr/lib/rexon/rexon-main-window"), "");
        assert_eq!(repo_root_from_exe(""), "");
        // Похоже, но не то: `target` без `rust` над ним.
        assert_eq!(repo_root_from_exe("/opt/x/target/release/rexon"), "");
    }

    /// Файл значка обязан находиться по корню, который посчитала функция
    /// выше: связку «корень → кандидаты» легко разорвать врозь.
    #[test]
    fn кандидаты_значка_собираются_из_посчитанного_корня() {
        english();
        let root = repo_root_from_exe("/tree/rust/target/release/rexon-main-window");
        let candidates = icon_candidates(&root);
        assert_eq!(candidates[0], "/tree/rexon_icon_256.png");
        // Пустой корень не добавляет мёртвых путей — остаются только
        // системные.
        let system = icon_candidates("");
        assert!(system.iter().all(|p| p.starts_with("/usr/")));
    }
}
