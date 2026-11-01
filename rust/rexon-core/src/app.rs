//! Порт чистой части `rexon/app.py` — точки входа и аварийного отката KDE.
//!
//! В Python модуль перемешивает решения с исполнением: запускает
//! `kwriteconfig`, переписывает конфиги, дёргает plasmashell. Сюда переехало
//! то, что от системы НЕ зависит: разбор аргументов командной строки в
//! маршрут, распознавание следов форк-темы, хирургия раскладки панели
//! (чистое преобразование текста appletsrc) и argv команд сброса.
//!
//! Аварийный откат `--recover-kde` должен работать даже когда Qt сломан,
//! поэтому ни ядро, ни исполнитель этой ветки к Qt не обращаются.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Mutex;

use crate::i18n::{fill, n_, ntr, tr, trf};

// ============================================================================
// Строка-константа, которая переводится в момент показа
// ============================================================================

/// Строка для человека, которая живёт КОНСТАНТОЙ (`app::FONTS_RESET_LINE`),
/// но выходит на экран уже переведённой — на язык, выбранный к моменту
/// показа.
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ ТИП. Эти константы берут исполнитель отката
/// (`system_exec`), заставка (`session_splash`), лоток и сверки, и все они
/// передают их туда, где ждут `&str` (`out(app::FONTS_RESET_LINE)`), или
/// сравнивают со строкой. `&TrLine` сам приводится к `&str` (через
/// [`std::ops::Deref`]) и сравнивается со `String`, поэтому переводу не
/// пришлось переписывать ни одного места показа. Внутри лежит `msgid`
/// (помечен [`n_`] — его видит `xgettext`), перевод берётся [`tr`] на каждом
/// обращении.
///
/// Цена приведения к `&str` — переведённый текст однажды оседает в памяти
/// процесса навсегда (одна копия на пару «строка × язык», а строк — десяток).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrLine(&'static str);

impl TrLine {
    /// Константа из `msgid`; сам `msgid` пометить `n_(…)`.
    pub const fn new(msgid: &'static str) -> Self {
        Self(msgid)
    }

    /// Исходная (английская) строка — ключ каталога.
    pub const fn msgid(&self) -> &'static str {
        self.0
    }

    /// Переведённый текст.
    pub fn text(&self) -> String {
        tr(self.0)
    }
}

/// Переведённый текст с временем жизни программы. Одинаковые тексты не
/// копятся: второй раз отдаётся уже осевшая копия.
fn interned(text: String) -> &'static str {
    static SEEN: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    // Отравленный замок (паника в чужом потоке посреди вставки) аварийному
    // маршруту не помеха: множество от этого не портится.
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = seen.get(text.as_str()) {
        return known;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    seen.insert(leaked);
    leaked
}

impl std::ops::Deref for TrLine {
    type Target = str;
    fn deref(&self) -> &str {
        interned(self.text())
    }
}

impl AsRef<str> for TrLine {
    fn as_ref(&self) -> &str {
        self
    }
}

impl fmt::Display for TrLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl PartialEq<TrLine> for str {
    fn eq(&self, other: &TrLine) -> bool {
        self == other.text()
    }
}

impl PartialEq<TrLine> for String {
    fn eq(&self, other: &TrLine) -> bool {
        *self == other.text()
    }
}

impl PartialEq<&TrLine> for String {
    fn eq(&self, other: &&TrLine) -> bool {
        *self == other.text()
    }
}

// ============================================================================
// Маршрут командной строки
// ============================================================================

/// Одиночные Qt-вкладки: флаг → модуль-исполнитель. Порядок совпадает с
/// оригиналом; на нём же держится приоритет при нескольких флагах сразу.
pub const SINGLE_TAB_FLAGS: &[(&str, &str)] = &[
    ("--appearance-qt", "rexon.appearance_page"),
    ("--extensions-qt", "rexon.qt_extensions.app"),
    ("--compare-qt", "rexon.qt_compare.app"),
    ("--settings-qt", "rexon.qt_settings.app"),
    ("--desktops-qt", "rexon.qt_desktops.app"),
];

/// Что делать по разобранной командной строке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `--apply-defaults`: autostart-хук, применить DE-defaults сессии.
    ApplyDefaults,
    /// `--sanitize-gtk`: починить ссылки в gtk-{3,4}.0/settings.ini.
    SanitizeGtk,
    /// Одиночная Qt-вкладка; поле — модуль, у которого зовётся `main`.
    SingleTab { module: &'static str },
    /// `--session-splash`: заставка при входе (Qt не поднимается).
    SessionSplash,
    /// `--recover-kde`: аварийный откат Global Theme.
    RecoverKde,
    /// `--aeroshell-check [--login]`: проверка AeroThemePlasma без окна —
    /// её зовёт пользовательский юнит после обновления пакетов и при входе.
    AeroshellCheck { login: bool },
    /// `--update-check [--login]`: фоновая проверка «до и после обновления»
    /// ([`crate::update_guard`]) — её зовёт пользовательский юнит; при
    /// стоящих форках Plasma она ведёт и к сверке AeroThemePlasma.
    UpdateCheck { login: bool },
    /// `--conky-set <имя>`: применить набор виджетов Conky без окна
    /// ([`crate::conky_sets::cli_route`]).
    ConkySet,
    /// `--apply-day-night [--login]`: светлое или тёмное по расписанию
    /// ([`crate::day_night::cli_route`]) — его зовут таймер и автозапуск.
    DayNight,
    /// `rexon doctor`: почему не поднимается экран входа и что делать —
    /// только смотрит ([`crate::login_rescue::doctor_live`]).
    Doctor,
    /// `--help` / `-h`: справка по командной строке — печатается и только.
    Help,
    /// `--version` / `-V`: версия Rexon одной строкой.
    Version,
    /// Без флагов либо `--qt`: единое окно с вкладками.
    MainWindow,
}

/// Флаги справки. Справка и версия — раньше всех маршрутов и без окна:
/// `rexon --help` зовут из текстовой консоли и по SSH, где дисплея нет, и
/// до 30.10.2026 он проваливался в главное окно — Qt не находил платформу и
/// процесс падал с `abort` вместо справки.
pub const HELP_FLAGS: &[&str] = &["--help", "-h"];

/// Справка словом, как `rexon doctor`: `rexon help`.
pub const HELP_WORD: &str = "help";

/// Флаги версии (см. [`HELP_FLAGS`]).
pub const VERSION_FLAGS: &[&str] = &["--version", "-V"];

/// Разобрать аргументы в маршрут. Порядок проверок — как в оригинале:
/// служебные хуки раньше UI, `--recover-kde` последним из флагов, потому
/// что он самый тяжёлый и не должен перехватывать более узкие режимы.
///
/// `args` — аргументы БЕЗ имени программы (`sys.argv[1:]`); оригинал ищет
/// флаги по всему `sys.argv`, но имя программы флагом быть не может.
pub fn route(args: &[String]) -> Route {
    let has = |flag: &str| args.iter().any(|a| a == flag);

    // Справка и версия побеждают всё: `rexon --recover-kde --help` обязан
    // рассказать про откат, а не выполнить его. Слово `help` — только
    // первым, как `doctor`: иначе имя набора Conky «help» стало бы справкой.
    if args.first().is_some_and(|a| a == HELP_WORD) || HELP_FLAGS.iter().any(|f| has(f)) {
        return Route::Help;
    }
    if VERSION_FLAGS.iter().any(|f| has(f)) {
        return Route::Version;
    }

    if has("--apply-defaults") {
        return Route::ApplyDefaults;
    }
    if has("--sanitize-gtk") {
        return Route::SanitizeGtk;
    }
    for (flag, module) in SINGLE_TAB_FLAGS {
        if has(flag) {
            return Route::SingleTab { module };
        }
    }
    if has("--session-splash") {
        return Route::SessionSplash;
    }
    if has(RECOVER_KDE_FLAG) {
        return Route::RecoverKde;
    }
    if has(crate::aeroshell::CHECK_FLAG) {
        return Route::AeroshellCheck {
            login: has(crate::aeroshell::LOGIN_FLAG),
        };
    }
    if args.iter().any(|a| a == crate::conky_sets::CLI_FLAG || a.starts_with(&format!("{}=", crate::conky_sets::CLI_FLAG))) {
        return Route::ConkySet;
    }
    if has(crate::day_night::CLI_FLAG) {
        return Route::DayNight;
    }
    if crate::login_rescue::is_doctor_args(args) {
        return Route::Doctor;
    }
    if has(crate::update_guard::CHECK_FLAG) {
        return Route::UpdateCheck {
            login: has(crate::aeroshell::LOGIN_FLAG),
        };
    }
    Route::MainWindow
}

/// Флаг аварийного отката оформления. Один на разбор маршрута и на окно,
/// которое этот маршрут запускает ([`crate::window_menu::recover_argv`]):
/// две копии флага разъехались бы молча, и пункт меню звал бы маршрут,
/// которого нет.
pub const RECOVER_KDE_FLAG: &str = "--recover-kde";

/// Аргументы, передаваемые главному окну: `--qt` — исторический алиас
/// «поведения по умолчанию» и до окна не доходит.
pub fn main_window_args(args: &[String]) -> Vec<String> {
    args.iter().filter(|a| *a != "--qt").cloned().collect()
}

// ============================================================================
// Ярлыки рабочего стола: `--page=<имя>`
// ============================================================================
//
// ДЕФЕКТ, КОТОРЫЙ ЭТО ЗАКРЫВАЕТ. Ярлык Rexon держит три быстрых действия
// (`installer::desktop_content`, они же в `packaging/aur/rexon.desktop`):
// «Установить рабочий стол» — `rexon --page=install`, «Управление
// резервными копиями» — `rexon --page=backups`, «Сравнить рабочие столы» —
// `rexon --page=compare`. Флаг `--page=` НЕ разбирал никто: ни `route`
// здесь, ни Python. `route` отдавал `MainWindow`, `main_window_args`
// пропускал строку дальше нетронутой, Qt незнакомый аргумент молча
// игнорировал — и все три пункта открывали одно и то же окно на разделе по
// умолчанию. Нажатие правой кнопкой по ярлыку → «Управление резервными
// копиями» приводило человека в «Рабочие столы», а не в резервные копии;
// «Сравнить» — туда же. Обещание в меню есть, поведения за ним нет, и это
// ровно тот молчаливый обман, который правило #13 запрещает.
//
// Таблица живёт ЗДЕСЬ, потому что имена в ярлыке пишет соседний модуль
// (`installer::desktop_content`) — и он, и разбор обязаны читать один
// список. Сторож `ярлык_не_обещает_неизвестных_страниц` в
// `tests/app_parity.rs` краснеет, если в ярлыке появится `--page=` без
// записи здесь.

/// Префикс флага ярлыка: `--page=install`.
pub const PAGE_FLAG: &str = "--page=";

/// Имя из ярлыка → (раздел главного окна, подраздел внутри него).
///
/// Раздел — идентификатор из [`crate::qt_main::SECTIONS`]; своей копии
/// состава разделов здесь нет, а сверка обоих списков — в тестах.
/// Подраздел пуст, когда открывать нужно сам раздел.
///
/// «Сравнение DE» с 08.07.2026 живёт ВНУТРИ «Рабочих столов», поэтому у
/// `compare` тот же раздел, что у `install`, и отличает их подраздел.
pub const PAGE_TARGETS: &[(&str, &str, &str)] = &[
    ("install", "desktops", ""),
    ("backups", "settings", ""),
    ("compare", "desktops", "compare"),
];

/// Куда открывать окно по `--page=…`: (раздел, подраздел).
///
/// `None` — флага нет вовсе либо имя незнакомое. Незнакомое имя намеренно
/// не считается ошибкой запуска: окно обязано открыться хоть на разделе по
/// умолчанию, а не отказать человеку из-за опечатки в чужом ярлыке.
pub fn start_page(args: &[String]) -> Option<(&'static str, &'static str)> {
    let name = args
        .iter()
        .find_map(|a| a.strip_prefix(PAGE_FLAG))?
        .trim()
        .to_lowercase();
    PAGE_TARGETS
        .iter()
        .find(|(page, _, _)| *page == name)
        .map(|(_, section, subpage)| (*section, *subpage))
}

// ============================================================================
// Чем заводить Rexon — одна правда на все строки запуска, уезжающие на диск
// ============================================================================

/// Программа для строки запуска, которую Rexon записывает на диск: `Exec=`
/// автозапуска и ярлыка, `ExecStart=` systemd-юнита. `None` — заводить
/// НЕЧЕМ, и тогда строку писать нельзя: мёртвая запись оживает при
/// следующем входе в сеанс, когда объяснить поломку уже некому (правило
/// #13 — отказывать вслух, а не рапортовать успех).
///
/// ⚠️ ОДНО МЕСТО НА ПРОДУКТ (правило #12). Сюда сходятся все три писателя
/// строк запуска: лоток ([`crate::tray::autostart_exec`]), заставка входа
/// ([`crate::session_splash::desktop_launcher_exec`]) и автозапуск
/// дефолтных приложений ([`crate::appdefaults::autostart_exec_for`]).
/// Разъехаться им нельзя: замок «один Rexon на сессию» у всех записей
/// общий, и запись, заводящая ДРУГОЙ вход, превращается в процесс, который
/// стучится в занятый замок и молча выходит.
///
/// Сам вход выбирает [`crate::installer::launch_entry_live`] — тоже одна
/// функция на всех, и она знает все раскладки, где бинарь окна бывает:
/// каталоги `PATH`, пакет дистрибутива, установка в систему, установка в
/// домашнюю папку, дерево разработчика за симлинком установленного
/// каталога ([`crate::installer::main_window_candidates`]). Своего поиска
/// бинаря здесь нет и заводить его нельзя.
///
/// ⛔ Запасного пути «поднять интерпретатор и позвать модуль» здесь НЕТ.
/// Он жил у лотка и у заставки до 16.09.2026 (`python3 -m rexon.app …`) и
/// был мёртв по построению: строка из `.desktop`-записи автозапуска
/// выполняется без какого-либо рабочего каталога, а пакет `rexon` в
/// `sys.path` системного интерпретатора не лежит — вход в сеанс молча не
/// делал ничего. Тот же дефект уже чинили у дефолтных приложений
/// (`appdefaults_exec`, пункт 1 шапки).
///
/// ⭐ Починка при переносе: путь уходил в `Exec=` без кавычек, и каталог с
/// пробелом разрывал команду на два аргумента.
pub fn launch_program(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
) -> Option<String> {
    use crate::installer::{desktop_exec_arg, LaunchEntry};
    match entry {
        LaunchEntry::MainWindow(_) => Some(entry.exec_program()),
        LaunchEntry::Python => nonempty(rexon_path).map(desktop_exec_arg),
    }
}

/// [`launch_program`] со своими флагами: `<программа> <флаг>…`. Флаги
/// кавычек не требуют — это наши собственные слова без пробелов.
pub fn launch_command(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    args: &[&str],
) -> Option<String> {
    let mut out = launch_program(entry, rexon_path)?;
    for a in args {
        out.push(' ');
        out.push_str(a);
    }
    Some(out)
}

/// То же для `ExecStart=` systemd-юнита. Отдельно от [`launch_command`]
/// только из-за кавычек: у Desktop Entry значение ключа проходит
/// разэкранирование ДО разбора аргументов (косая удваивается ещё раз), а
/// systemd лишнюю косую прочёл бы буквально
/// ([`crate::session_splash::systemd_exec_arg`]).
pub fn unit_launch_command(
    entry: &crate::installer::LaunchEntry,
    rexon_path: Option<&str>,
    args: &[&str],
) -> Option<String> {
    use crate::installer::LaunchEntry;
    let program = match entry {
        LaunchEntry::MainWindow(path) => path.as_str(),
        LaunchEntry::Python => nonempty(rexon_path)?,
    };
    let mut out = crate::session_splash::systemd_exec_arg(program);
    for a in args {
        out.push(' ');
        out.push_str(a);
    }
    Some(out)
}

/// Непустое значение без окружающих пробелов; пустая строка — это «не
/// нашли», а не путь (`shutil.which` оригинала возвращал `None`, мост и
/// исполнители — пустую строку).
fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// Отказ, когда заводить Rexon нечем: бинаря окна не нашлось ни в одной
/// раскладке, лаунчера `rexon` в `PATH` тоже нет.
pub const MSG_NOTHING_TO_LAUNCH: &TrLine = &TrLine::new(n_(
    "Couldn't find anything to start Rexon with: no window binary (package, system \
     install, install in the home folder, developer tree) and no `rexon` launcher in \
     PATH. The autostart entry wasn't created — it would point to nothing.",
));

// ============================================================================
// Следы форк-темы (Aero и родня)
// ============================================================================

/// Отличительные токены форк-темы. НЕ голые `aero`/`win7`: те ловят
/// легитимные строки вроде имени обоев и зря триггерят сброс панели.
pub const FORK_TOKENS: &[&str] = &[
    "wackyideas",
    "aeroshell",
    "aerothemeplasma",
    "sevenstart",
    "se7enaero",
    "win7os",
];

/// Есть ли в строке токен форк-темы (регистр не важен).
pub fn has_fork_token(text: &str) -> bool {
    let lower = text.to_lowercase();
    FORK_TOKENS.iter().any(|t| lower.contains(t))
}

/// Файлы, где ищем следы форка: раскладка панели и конфиги SDDM.
/// Раскрытие маски `/etc/sddm.conf.d/*.conf` — за исполнителем.
pub fn fork_residue_files(home: &str) -> (String, &'static str, &'static str) {
    (appletsrc_path(home), "/etc/sddm.conf.d", "/etc/sddm.conf")
}

/// Путь раскладки рабочего стола и панелей Plasma.
pub fn appletsrc_path(home: &str) -> String {
    appletsrc_path_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME` (единый корень конфигов ядра).
pub fn appletsrc_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/plasma-org.kde.plasma.desktop-appletsrc",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Путь бэкапа, который кладётся рядом перед хирургией панели.
pub fn appletsrc_backup_path(appletsrc: &str) -> String {
    format!("{appletsrc}.rexon-bak")
}

/// Убрать из текста appletsrc группы апплетов форк-темы.
///
/// Файл KConfig разбивается на секции «заголовок + строки до следующего
/// заголовка»; секция считается чужой, если в ней есть `plugin=` с токеном
/// форка. Такие секции выкидываются ВМЕСТЕ с вложенными подсекциями
/// (`[…][Configuration]` и глубже) — иначе осиротевшая конфигурация
/// остаётся мусором в файле.
///
/// Остальные апплеты, systemtray и его `extraItems`/`knownItems` не
/// трогаются: стоковые громкость, батарея и сеть вернутся сами.
///
/// Возвращает `(новый текст, удалённые plugin)`; если форка нет — текст
/// отдаётся байт-в-байт исходным.
pub fn strip_fork_applets(text: &str) -> (String, Vec<String>) {
    // Разбиение на секции: (заголовок, строки). Преамбула до первого
    // заголовка идёт с `None` и никогда не выкидывается.
    let mut sections: Vec<(Option<String>, Vec<&str>)> = Vec::new();
    let mut cur_header: Option<String> = None;
    let mut cur_lines: Vec<&str> = Vec::new();

    for line in split_keepends(text) {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            sections.push((cur_header.take(), std::mem::take(&mut cur_lines)));
            cur_header = Some(s.to_string());
            cur_lines.push(line);
        } else {
            cur_lines.push(line);
        }
    }
    sections.push((cur_header, cur_lines));

    // Заголовки секций, чей plugin принадлежит форку.
    let mut fork_headers: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for (header, body) in &sections {
        let Some(header) = header else { continue };
        for line in body {
            let st = line.trim();
            if let Some(plugin) = st.strip_prefix("plugin=") {
                if has_fork_token(st) {
                    fork_headers.push(header.clone());
                    removed.push(plugin.to_string());
                    break;
                }
            }
        }
    }

    if fork_headers.is_empty() {
        return (text.to_string(), Vec::new());
    }

    let mut out = String::with_capacity(text.len());
    for (header, body) in &sections {
        if let Some(h) = header {
            if fork_headers.iter().any(|fh| h.starts_with(fh.as_str())) {
                continue; // выкинуть секцию вместе с подсекциями
            }
        }
        for line in body {
            out.push_str(line);
        }
    }
    (out, removed)
}

/// Порт `str.splitlines(keepends=True)`: строки вместе с их окончаниями.
fn split_keepends(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            out.push(&text[start..=i]);
            i += 1;
            start = i;
        } else if bytes[i] == b'\r' {
            let end = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i + 1
            } else {
                i
            };
            out.push(&text[start..=end]);
            i = end + 1;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        out.push(&text[start..]);
    }
    out
}

// ============================================================================
// argv команд сброса
// ============================================================================

/// Ключи kdeglobals, снимаемые при сбросе шрифтов. `--delete` снимает ключ,
/// после чего Plasma берёт собственный дефолт — надёжнее, чем хардкодить
/// конкретный шрифт.
pub const KDE_FONT_KEYS: &[&str] = &[
    "font",
    "fixed",
    "smallestReadableFont",
    "toolBarFont",
    "menuFont",
    "activeFont",
];

/// Эффекты KWin от форк-тем, снимаемые поимённо. Чужие и пользовательские
/// эффекты не трогаются — удаляются только эти ключи `<эффект>Enabled`.
pub const KWIN_FORK_EFFECTS: &[&str] = &[
    "aeroglassblur",
    "aeroglide",
    "dimscreenaero",
    "smod",
    "smodglow",
    "smodanimations",
    "aerosnap",
];

/// argv снятия всех переопределений шрифтов (по вызову на ключ).
pub fn kde_font_reset_argv(kwriteconfig: &str) -> Vec<Vec<String>> {
    KDE_FONT_KEYS
        .iter()
        .map(|key| {
            vec![
                kwriteconfig.to_string(),
                "--file".into(),
                "kdeglobals".into(),
                "--group".into(),
                "General".into(),
                "--key".into(),
                (*key).to_string(),
                "--delete".into(),
            ]
        })
        .collect()
}

/// argv снятия эффектов KWin от форк-тем (по вызову на эффект).
pub fn kwin_effect_reset_argv(kwriteconfig: &str) -> Vec<Vec<String>> {
    KWIN_FORK_EFFECTS
        .iter()
        .map(|eff| {
            vec![
                kwriteconfig.to_string(),
                "--file".into(),
                "kwinrc".into(),
                "--group".into(),
                "Plugins".into(),
                "--key".into(),
                format!("{eff}Enabled"),
                "--delete".into(),
            ]
        })
        .collect()
}

/// argv перечитывания конфигурации живым KWin.
pub fn kwin_reconfigure_argv(qdbus: &str) -> Vec<String> {
    vec![
        qdbus.to_string(),
        "org.kde.KWin".into(),
        "/KWin".into(),
        "org.kde.KWin.reconfigure".into(),
    ]
}

/// argv возврата стиля виджетов на Breeze.
pub fn widget_style_breeze_argv(kwriteconfig: &str) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "kdeglobals".into(),
        "--group".into(),
        "KDE".into(),
        "--key".into(),
        "widgetStyle".into(),
        "Breeze".into(),
    ]
}

/// Похожа ли тема Kvantum на Aero-семейство. Легитимные темы (Dracula и
/// прочие) не трогаем — только эти признаки.
pub fn is_aero_kvantum_theme(theme: &str) -> bool {
    let t = theme.to_lowercase();
    ["aero", "seven", "win7", "windows7"]
        .iter()
        .any(|m| t.contains(m))
}

/// Кандидаты бинаря kwriteconfig в порядке предпочтения.
pub const KWRITECONFIG_BINS: &[&str] = &["kwriteconfig6", "kwriteconfig5"];
/// Кандидаты бинаря qdbus в порядке предпочтения.
pub const QDBUS_BINS: &[&str] = &["qdbus6", "qdbus"];

// ============================================================================
// --recover-kde: возврат обоев рабочих столов к стандартным
// ============================================================================

/// Каталоги обоев-кандидатов на роль «стандартных»: сначала штатный Plasma
/// Next, потом наборы CachyOS (основная машина). Ни один не найден —
/// исполнитель ищет любой пакет обоев сам.
pub const DEFAULT_WALLPAPER_DIRS: &[&str] = &[
    "/usr/share/wallpapers/Next",
    "/usr/share/wallpapers/cachyos-wallpapers",
    "/usr/share/wallpapers/Cachy",
];

/// Контейнмент, на который ставим обои, если в appletsrc не нашлось ни
/// одного десктопного (у Plasma это первый номер по умолчанию).
pub const FALLBACK_CONTAINMENT: &str = "128";

/// Номера ДЕСКТОПНЫХ контейнментов из текста appletsrc — порядок появления,
/// без повторов.
///
/// Секция `[Containments][<N>]` запоминается, и если В НЕЙ САМОЙ встречается
/// строка `plugin=…`, содержащая `folder` или `desktopcontainment`, номер
/// попадает в список. Панели (`plugin=org.kde.panel`) так отсеиваются сами.
///
/// ⚠️ Починка бага оригинала (правило #16): Python не сбрасывал «текущий
/// контейнмент» на чужом заголовке секции, поэтому `plugin=` ЛЮБОЙ
/// вложенной секции засчитывался предыдущему контейнменту. Апплет «Папки»
/// (`org.kde.plasma.folder`), положенный в панель, делал ПАНЕЛЬ «десктопом»
/// — и аварийный сброс писал ей `wallpaperplugin`/`Image` и завышал число
/// «рабочих столов» в отчёте. Здесь любой не-контейнментный заголовок
/// закрывает секцию.
pub fn desktop_containment_ids(appletsrc_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for line in appletsrc_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            cur = containment_header_id(trimmed);
            continue;
        }
        if let Some(id) = cur.as_ref() {
            if line.starts_with("plugin=")
                && (line.contains("folder") || line.contains("desktopcontainment"))
                && !out.iter().any(|x| x == id)
            {
                out.push(id.clone());
            }
        }
    }
    out
}

/// Номер из строки ровно вида `[Containments][<цифры>]` (порт регулярного
/// выражения `^\[Containments\]\[(\d+)\]$`). Вложенные секции
/// (`[Containments][2][Applets][5]`) намеренно НЕ подходят: у апплета свой
/// `plugin=`, и он не должен выдавать себя за десктоп.
fn containment_header_id(line: &str) -> Option<String> {
    const PREFIX: &str = "[Containments][";
    let rest = line.strip_prefix(PREFIX)?.strip_suffix(']')?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(rest.to_string())
}

/// argv возврата плагина обоев контейнмента на штатную картинку.
pub fn wallpaper_plugin_reset_argv(kwriteconfig: &str, containment: &str) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "plasma-org.kde.plasma.desktop-appletsrc".into(),
        "--group".into(),
        "Containments".into(),
        "--group".into(),
        containment.to_string(),
        "--key".into(),
        "wallpaperplugin".into(),
        "org.kde.image".into(),
    ]
}

/// argv записи самой картинки обоев контейнмента. `wallpaper_dir` —
/// каталог пакета обоев СО слэшем на конце (так его пишет сам Plasma).
pub fn wallpaper_image_argv(
    kwriteconfig: &str,
    containment: &str,
    wallpaper_dir: &str,
) -> Vec<String> {
    vec![
        kwriteconfig.to_string(),
        "--file".into(),
        "plasma-org.kde.plasma.desktop-appletsrc".into(),
        "--group".into(),
        "Containments".into(),
        "--group".into(),
        containment.to_string(),
        "--group".into(),
        "Wallpaper".into(),
        "--group".into(),
        "org.kde.image".into(),
        "--group".into(),
        "General".into(),
        "--key".into(),
        "Image".into(),
        wallpaper_dir.to_string(),
    ]
}

/// argv перезапуска plasmashell, чтобы записанные обои подхватились.
/// `reset-failed` первым — иначе повторный restart после краха отвергается.
pub fn plasmashell_restart_argv() -> Vec<Vec<String>> {
    vec![
        vec![
            "systemctl".into(),
            "--user".into(),
            "reset-failed".into(),
            "plasma-plasmashell.service".into(),
        ],
        vec![
            "systemctl".into(),
            "--user".into(),
            "restart".into(),
            "plasma-plasmashell.service".into(),
        ],
    ]
}

/// Подсказка по установке PySide6 — печатается, когда Qt-стек не найден.
pub const PYSIDE6_INSTALL_HINT: &[&str] = &[
    "Arch:   sudo pacman -S pyside6",
    "Debian: sudo apt install python3-pyside6",
    "Fedora: sudo dnf install python3-pyside6",
];

// ============================================================================
// Один Rexon на сессию: второй запуск не плодит второй трей-значок
// ============================================================================
//
// Механизм целиком переехал в `rexon_core::single_instance`: там же, где
// адреса замка и сокета, теперь живут и сам `flock` (он лежал в
// Python-биндинге и Rust-точке входа был недоступен), и слушатель «подними
// окно», и стук в него. Разбор дефектов, из которых всё это выросло, — в
// шапке того модуля.
//
// Здесь остаются только прежние имена: `rexon/app.py` зовёт биндинг по ним,
// и переименование ради переезда сломало бы живой продукт на ровном месте.

pub use crate::single_instance::lock_path as single_instance_lock_path;
pub use crate::single_instance::socket_path as single_instance_socket_path;

// ============================================================================
// --recover-kde: решения и тексты аварийного отката
// ============================================================================
//
// ЗАЧЕМ ЭТА ЧАСТЬ ЗДЕСЬ. Порядок шагов и походы наружу живут в исполнителе
// (`system_exec::SystemExec::recover_kde`), а тут — всё, что решается БЕЗ
// системы: какую тему применяем, считается ли текущий рабочий стол
// Plasma, argv каждой команды и каждая строка, которую человек увидит.
// Разделение то же, что у соседних маршрутов: тексты закреплены сверками
// (`tests/app_parity.rs` читает `rexon/app.py` через `include_str!`), и
// вторая копия слов в исполнителе разъехалась бы с оригиналом молча.
//
// ⚠️ Маршрут аварийный: его зовут, когда оформление уже сломано, и он
// обязан работать без Qt вовсе. Поэтому ни одна строка отсюда не
// обращается к Qt. Перевод — через `crate::i18n`, а он Qt не касается:
// каталог `.mo` читается чистым Rust, и если каталога нет или он битый,
// `tr` отдаёт исходную английскую строку, без паники. Аварийный маршрут
// при этом говорит по-английски, а не молчит.

/// Тема, которая применяется, если резервной копии нет.
pub const RECOVER_DEFAULT_TARGET: &str = "org.kde.breezedark.desktop";

/// Цветовая схема, возвращаемая вслед за темой.
pub const RECOVER_COLORSCHEME: &str = "BreezeDark";

/// Признаки Plasma в `$XDG_CURRENT_DESKTOP` (регистр не важен).
pub const PLASMA_DESKTOP_MARKERS: &[&str] = &["kde", "plasma"];

/// Можно ли восстанавливать оформление в ЭТОМ сеансе.
///
/// Ярлык назван нейтрально («Восстановить оформление рабочего стола»), но
/// само восстановление умеет только Plasma: оно откатывает Global Theme,
/// раскладку панели, SDDM и Plymouth. На чужом рабочем столе честно
/// отказываемся, а не делаем вид, что сработали (правило #13).
///
/// Пустая переменная — это НЕ «чужой рабочий стол», а «неизвестно»
/// (запуск из tty, из systemd-юнита, из терминала без сеанса): там
/// отказывать нельзя, иначе аварийный маршрут перестанет работать ровно в
/// том случае, ради которого он и заведён — когда сеанс уже не поднимается.
pub fn is_plasma_desktop(xdg_current_desktop: &str) -> bool {
    if xdg_current_desktop.trim().is_empty() {
        return true;
    }
    let lower = xdg_current_desktop.to_lowercase();
    PLASMA_DESKTOP_MARKERS.iter().any(|m| lower.contains(m))
}

/// Что сказать человеку на чужом рабочем столе (три строки подряд).
pub fn not_plasma_lines(xdg_current_desktop: &str) -> Vec<String> {
    vec![
        tr("⚠️  Emergency appearance recovery only supports KDE Plasma for now."),
        format!(
            "    {}",
            trf("Current desktop: %1. Nothing was changed.", &[&xdg_current_desktop])
        ),
        format!(
            "    {}",
            tr("On this desktop, roll the appearance back with the desktop's own tools \
                or on the Appearance tab in Rexon.")
        ),
    ]
}

/// Какую тему восстанавливаем: сохранённую перед применением либо
/// [`RECOVER_DEFAULT_TARGET`].
///
/// ⚠️ Не путать с [`crate::window_menu::recover_target`]: та отвечает на
/// вопрос ДИАЛОГА («что будет применено» — там пустая копия называется
/// словами «Breeze Dark (fallback)»), а эта — на вопрос КОМАНДЫ, и её ответ
/// уходит в argv, где человеческое имя не годится.
pub fn recover_theme_target(saved: Option<&str>) -> String {
    match saved.map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => RECOVER_DEFAULT_TARGET.to_string(),
    }
}

/// Сколько ждём применения Global Theme (пересчёт кешей Plasma не быстрый).
pub const LOOKANDFEEL_TIMEOUT_SECS: u64 = 30;
/// Сколько ждём применения цветовой схемы.
pub const COLORSCHEME_TIMEOUT_SECS: u64 = 15;
/// Сколько ждём одиночную правку конфига `kwriteconfig`.
pub const KWRITECONFIG_TIMEOUT_SECS: u64 = 10;
/// Сколько ждём полный сброс раскладки панели.
pub const PANEL_RESET_TIMEOUT_SECS: u64 = 30;
/// Сколько ждём ответа сессионной шины на `reconfigure`.
pub const KWIN_RECONFIGURE_TIMEOUT_SECS: u64 = 10;
/// Таймауты перезапуска plasmashell — по команде из
/// [`plasmashell_restart_argv`] (сброс отметки быстрый, сам рестарт нет).
pub const PLASMASHELL_RESTART_TIMEOUTS: &[u64] = &[5, 15];

/// argv применения Global Theme.
pub fn lookandfeel_apply_argv(target: &str) -> Vec<String> {
    vec![
        "plasma-apply-lookandfeel".to_string(),
        "-a".to_string(),
        target.to_string(),
    ]
}

/// argv применения цветовой схемы вслед за темой.
pub fn colorscheme_apply_argv() -> Vec<String> {
    vec![
        "plasma-apply-colorscheme".to_string(),
        RECOVER_COLORSCHEME.to_string(),
    ]
}

/// argv полного сброса раскладки панели. ⚠️ Стирает ВСЕ пользовательские
/// панели и виджеты — вызывается только по следам форк-темы.
pub fn panel_reset_layout_argv(tool: &str, target: &str) -> Vec<String> {
    vec![
        tool.to_string(),
        "-a".to_string(),
        target.to_string(),
        "--resetLayout".to_string(),
    ]
}

/// Предупреждение про пакеты-форки Plasma: заголовок, список, и — смотря
/// по тому, собралась ли команда полного отката, — либо она сама, либо
/// объяснение, почему её нет.
///
/// `preview_ok`/`preview` — ответ `plasma_fork_reset_preview`. Когда цели
/// нет в репозиториях, команду НЕ печатаем: человек выполнил бы её руками и
/// получил «не найдена цель».
pub fn fork_warning_lines(forks: &[String], preview_ok: bool, preview: &str) -> Vec<String> {
    let mut out = vec![tr(
        "⚠️  REPLACEMENT PACKAGES for Plasma components are installed (they replace \
         parts of Plasma itself, and a theme reset does NOT remove them):",
    )];
    for f in forks {
        out.push(format!("      • {f}"));
    }
    if preview_ok {
        out.push(format!("    {}", tr("To bring back standard Plasma COMPLETELY, remove them:")));
        out.push(format!("      {preview}"));
        out.push(format!(
            "    {}",
            tr("(then log out and log back in). The theme reset below only changes the \
                look — the replaced parts of Plasma stay.")
        ));
    } else {
        for line in preview.lines() {
            out.push(format!("    {line}"));
        }
    }
    out
}

/// «Начинаю» — печатается ДО применения темы.
pub fn recovering_line(target: &str) -> String {
    trf("🛟 Restoring the KDE theme: %1", &[&target])
}

/// «Получилось» — печатается сразу после успешного применения темы.
pub fn restored_line(target: &str) -> String {
    trf("✅ Restored: %1", &[&target])
}

/// Сколько символов чужого stderr показываем человеку.
pub const RECOVER_STDERR_LIMIT: usize = 200;

/// «Не получилось»: голова чужого stderr, обрезанная по
/// [`RECOVER_STDERR_LIMIT`].
///
/// Обрезка по СИМВОЛАМ, а не по байтам: срез посреди многобайтной буквы
/// (сообщения `plasma-apply-lookandfeel` переведены) в Rust — паника, то
/// есть аварийный маршрут падал бы ровно на неудаче, которую пришёл
/// объяснить.
pub fn apply_failed_line(stderr: &str) -> String {
    let head: String = stderr.chars().take(RECOVER_STDERR_LIMIT).collect();
    trf("❌ Error: %1", &[&head])
}

/// Итог сброса шрифтов.
pub const FONTS_RESET_LINE: &TrLine = &TrLine::new(n_("🔤 KDE fonts reset to the defaults."));

/// Итог снятия эффектов KWin.
pub const KWIN_EFFECTS_RESET_LINE: &TrLine =
    &TrLine::new(n_("🪟 KWin effects left by the Plasma replacements removed."));

/// В журнал, когда живой KWin конфиг не перечитал: правка легла в файл, но
/// на экране до следующего входа ничего не изменится — и человеку об этом
/// сказано, а не «молча ничего не произошло».
///
/// Журнал — не переводится (правило захода перевода).
pub const KWIN_RECONFIGURE_FAILED_LOG: &str =
    "KWin reconfigure не прошёл: шина не ответила, а бинаря qdbus в системе нет. \
     Эффекты снимутся при следующем входе в систему.";

/// Путь конфига Kvantum относительно корня конфигов.
pub const KVANTUM_CONFIG_REL: &str = "Kvantum/kvantum.kvconfig";
/// Секция и ключ темы в конфиге Kvantum.
pub const KVANTUM_THEME_SECTION: &str = "General";
/// Ключ темы в конфиге Kvantum.
pub const KVANTUM_THEME_KEY: &str = "theme";

/// Итог возврата стиля виджетов со Kvantum-Aero на Breeze.
pub fn kvantum_breeze_line(theme: &str) -> String {
    trf("🎨 Kvantum theme “%1” (Aero) → Breeze style.", &[&theme])
}

/// Родной дефолт Plymouth определить не удалось.
pub const PLYMOUTH_TARGET_UNKNOWN_LINE: &TrLine = &TrLine::new(n_(
    "⚠️ Couldn't determine the distribution's default Plymouth theme — skipping.",
));

/// Загрузочная заставка уже стоковая — ничего не делаем.
pub fn plymouth_already_stock_line(target: &str) -> String {
    trf("🐧 The boot splash is already the default one (%1).", &[&target])
}

/// Загрузочная заставка меняется (initramfs пересобирается).
pub fn plymouth_switch_line(current: &str, target: &str) -> String {
    trf(
        "🐧 Boot splash: “%1” → “%2” (rebuilding the initramfs; takes effect after a \
         reboot).",
        &[&current, &target],
    )
}

/// Ответ удавшегося шага (Plymouth и SDDM отвечают одинаково).
pub fn step_ok_line(message: &str) -> String {
    format!("   ✅ {message}")
}

/// Ответ неудавшегося сброса Plymouth.
pub fn plymouth_failed_line(message: &str) -> String {
    format!("   {}", trf("⚠️ Plymouth wasn't reset: %1", &[&message]))
}

/// Чем человек может сбросить заставку сам, если pkexec не прошёл.
pub fn manual_command_line(command: &str) -> String {
    format!("   {}", trf("To do it by hand: %1", &[&command]))
}

/// Экран входа уже на стоке. `theme` — дистро-дефолтная тема, если её
/// удалось назвать (человеку важно знать, к чему он вернётся).
pub fn sddm_already_stock_line(theme: Option<&str>) -> String {
    match theme {
        Some(t) if !t.is_empty() => trf(
            "🖥️ The login screen (SDDM) already has the standard settings (theme “%1”).",
            &[&t],
        ),
        _ => tr("🖥️ The login screen (SDDM) already has the standard settings."),
    }
}

/// Что именно удаляем из `/etc/sddm.conf.d` и к чему вернётся экран входа.
pub fn sddm_removing_line(to_remove: &[String], default_theme: Option<&str>) -> String {
    let files = to_remove.join(", ");
    match default_theme {
        Some(t) if !t.is_empty() => trf(
            "🖥️ Login screen (SDDM): removing third-party settings [%1] → it goes back to “%2”.",
            &[&files, &t],
        ),
        _ => trf(
            "🖥️ Login screen (SDDM): removing third-party settings [%1] → it goes back to \
             the distribution default.",
            &[&files],
        ),
    }
}

/// Ответ неудавшегося сброса экрана входа.
pub fn sddm_failed_line(message: &str) -> String {
    format!("   {}", trf("⚠️ SDDM wasn't reset: %1", &[&message]))
}

/// Найдены форки или их следы — переходим к чистке раскладки панели.
pub const PANEL_AGGRESSIVE_LINE: &TrLine = &TrLine::new(n_(
    "🔧 Plasma replacements or their leftovers found — cleaning them off the panel.",
));

/// Итог хирургии панели: что удалено и где лежит резервная копия.
pub fn applets_removed_line(removed: &[String], backup: &str) -> String {
    trf(
        "🧱 Broken widgets left by the Plasma replacements removed from the panel: %1 (%2). \
         Backup: %3",
        &[&removed.len(), &removed.join(", "), &backup],
    )
}

/// Панель переписать не удалось (нет прав, кончилось место).
pub fn applets_rewrite_failed_line(error: &str) -> String {
    trf("⚠️ Couldn't rewrite the panel: %1", &[&error])
}

/// Инструмента полного сброса раскладки в системе нет.
pub const LOOKANDFEELTOOL_MISSING_LINE: &TrLine = &TrLine::new(n_(
    "⚠️ lookandfeeltool not found — the panel layout wasn't reset.",
));

/// Итог полного сброса раскладки панели.
pub const PANEL_LAYOUT_RESET_LINE: &TrLine =
    &TrLine::new(n_("🧱 Panel layout reset to the default one (Kickoff)."));

/// Имя инструмента полного сброса раскладки панели.
pub const LOOKANDFEELTOOL_BIN: &str = "lookandfeeltool";

/// Каноническое имя правщика конфигов KDE (синонимы 6 → 5 знает
/// [`crate::utils::TOOL_SETS`]).
pub const KWRITECONFIG_TOOL: &str = "kwriteconfig6";

/// Где лежат пакеты обоев, поставленные системой.
pub const WALLPAPERS_ROOT: &str = "/usr/share/wallpapers";

/// Подкаталог пакета обоев, по которому пакет и опознаётся.
pub const WALLPAPER_PACKAGE_IMAGES: &str = "contents/images";

/// Каталог обоев в том виде, в каком его пишет сам Plasma — СО слэшем на
/// конце. Без слэша Plasma считает значение путём к одному файлу.
pub fn wallpaper_dir_value(dir: &str) -> String {
    format!("{}/", dir.trim_end_matches('/'))
}

/// Выбрать каталог «стандартных» обоев: сперва известные кандидаты
/// ([`DEFAULT_WALLPAPER_DIRS`]) в их порядке, потом ЛЮБОЙ установленный
/// пакет обоев по алфавиту. Пусто — обои не переписываем вовсе, только
/// возвращаем плагин на `org.kde.image`.
///
/// `is_dir` — проба каталога-кандидата, `packages` — имена в
/// [`WALLPAPERS_ROOT`] (порядок задаёт вызывающий; оригинал сортирует),
/// `has_images` — есть ли у пакета [`WALLPAPER_PACKAGE_IMAGES`].
pub fn default_wallpaper_dir(
    is_dir: &dyn Fn(&str) -> bool,
    packages: &[String],
    has_images: &dyn Fn(&str) -> bool,
) -> String {
    for cand in DEFAULT_WALLPAPER_DIRS {
        if is_dir(cand) {
            return wallpaper_dir_value(cand);
        }
    }
    for name in packages {
        let pkg = format!("{WALLPAPERS_ROOT}/{name}");
        if has_images(&format!("{pkg}/{WALLPAPER_PACKAGE_IMAGES}")) {
            return wallpaper_dir_value(&pkg);
        }
    }
    String::new()
}

/// Итог сброса обоев: сколько рабочих столов переписано и на что.
///
/// `wallpaper_dir` пуст — имени не называем: плагин вернули на картинку, а
/// самой картинки не нашлось, и врать про «сброшены на такие-то» нельзя.
pub fn wallpaper_reset_line(desktops: usize, wallpaper_dir: &str) -> String {
    let name = wallpaper_dir.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let n = desktops as u64;
    if wallpaper_dir.is_empty() || name.is_empty() {
        ntr(
            "🖼️  Wallpaper reset to the default one (%n desktop)",
            "🖼️  Wallpaper reset to the default one (%n desktops)",
            n,
        )
    } else {
        fill(
            &ntr(
                "🖼️  Wallpaper reset to the default one (%n desktop): %1",
                "🖼️  Wallpaper reset to the default one (%n desktops): %1",
                n,
            ),
            &[&name],
        )
    }
}

/// Применить тему нечем: `plasma-apply-lookandfeel` не запустился вовсе.
///
/// ⭐ Строки такого вида у оригинала не было: он звал `subprocess.run` без
/// проверки, и отсутствующий бинарь поднимал `FileNotFoundError` прямо из
/// `main()`. Аварийный маршрут падал трассировкой стека ровно там, где
/// обязан объяснить человеку, что происходит (правило #13).
pub const LOOKANDFEEL_MISSING_LINE: &TrLine = &TrLine::new(n_(
    "❌ Error: couldn't start plasma-apply-lookandfeel — KDE Plasma wasn't found on \
     this system. Nothing was changed.",
));
