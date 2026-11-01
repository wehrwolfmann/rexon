//! Compliance Check — проверка и автоисправление настроек DE, порт
//! `rexon/compliance.py` (класс `ComplianceCheck`).
//!
//! При запуске Rexon / переключении DE: определяется текущий DE, для него
//! прогоняется набор проверок (пакеты, gsettings, файлы, mimeapps), а
//! несоответствия автоматически чинятся. Поддержаны 8 DE: MATE, KDE, GNOME,
//! COSMIC, XFCE, Cinnamon, Budgie, Hyprland.
//!
//! Принцип порта (как `system_plan.rs` / `importers.rs`): вся ЧИСТАЯ логика —
//! детерминированные функции вход→выход (таблицы проверок, разбор состояния,
//! построение argv, решение «нужно ли чинить», генерация settings.ini и
//! bash-скрипта layout'ов). Побочные эффекты НЕ исполняются здесь: каждый
//! `safe_run`/`shutil`/`os`/`open`/`tempfile` оригинала — ровно один метод
//! трейта [`ComplianceIo`]. В parity-тестах трейт реплеит журнал живого
//! Python; в живом мосте — реальный I/O. Наблюдения (значение gsettings,
//! наличие пакета, содержимое файла, наличие helper'а) приходят из трейта
//! или параметром.
//!
//! Паритет БАЙТ-В-БАЙТ с Python: тот же argv (в т.ч. `pkexec …`), тот же
//! порядок проверок, тот же текст лога. Эталоны сняты с живого Python
//! read-only (`tests/golden/compliance_cases.json`); привилегированное НЕ
//! исполнялось.
//!
//! Сохранённые квирки оригинала (намеренное расхождение сохранено для
//! паритета):
//!   * `_check_mate` возвращает `mate_terminal="failed"` / `mate_cursor="failed"`
//!     при `gsettings get == None`, но при `== None` для локали cc уже
//!     `mate_cc_locale="failed"` — логика «gsettings недоступен» размазана,
//!     сохранена как есть;
//!   * `_check_xfce` для helpers.rc ВСЕГДА даёт `xfce_helpers="ok"` (даже
//!     когда `TerminalEmulator=` не найден или файла нет) — только текст лога
//!     меняется; сохранено;
//!   * `_patch_settings_ini`: если секции `[Settings]` не было, она
//!     вставляется в начало, затем `key=value` — на индекс `insert_at+1`
//!     (Python `list.insert` клампит выход за длину) — арифметика сохранена;
//!   * `detect_current_de` матчит первую часть `XDG_CURRENT_DESKTOP` (split
//!     по `:`), совпавшую с таблицей — порядок частей задаёт результат.
//!
//! Режим автозапуска (`allow_privileged = false`): при входе в систему Rexon
//! не имеет права спрашивать пароль и лезть в сеть. Диагностика при этом
//! выполняется полностью (честно смотрим, чего не хватает), а само лечение —
//! установка пакетов, AUR-хелпер, `git clone`, копирование в системные
//! каталоги через helper/`pkexec` — откладывается: шаг получает статус
//! `needs_action` и ОДНУ строку лога на весь прогон. Из интерфейса проверка
//! зовётся с `allow_privileged = true` и чинит как раньше.
//!
//! Гейт по семейству пакетного менеджера: имена пакетов в проверках арчевые
//! (`ttf-ubuntu-font-family`, `cosmic-wallpapers`, `ayatana-indicator-*`…),
//! а установка идёт через `pacman`/AUR-хелпер. На любом другом семействе
//! такие шаги НЕ выполняются вовсе (ни проверки пакетов, ни `pkexec`), получают
//! статус `skipped` и дают ОДНУ строку лога на весь прогон. Непакетные
//! проверки (gsettings, файлы конфигов, темы иконок) работают везде.
//! Семейство подаётся параметром — как `pkg_helper_available`.
//!
//! НЕ портировано (исполнение — при подключении к живому Python): реальные
//! `subprocess.run`/`safe_run`, `shutil.which/copy2/rmtree`, чтение/запись
//! `~/.config`, `tempfile.mkdtemp/mkstemp`, git clone layout'ов,
//! `pkexec` — всё выражено вызовами трейта. Наличие helper'ов
//! (`_pkg_helper_available`/`_stage_helper_available`) подаётся параметром.

use crate::i18n::{n_, ntr, tr, trc, trf};
use crate::system::PkgFamily;

// ============================================================================
// Константы-таблицы (порядок = как в Python, важен для порядка проверок/лога)
// ============================================================================

/// `ComplianceCheck.MATE_EXPECTED_LAYOUTS` — 8 layout'ов Ubuntu MATE.
pub const MATE_EXPECTED_LAYOUTS: &[&str] = &[
    "familiar", "redmond", "cupertino", "mutiny",
    "pantheon", "contemporary", "netbook", "traditional",
];

/// `ComplianceCheck.AYATANA_PACKAGES` — Ayatana-стек.
pub const AYATANA_PACKAGES: &[&str] = &[
    "ayatana-indicator-application",
    "ayatana-indicator-session",
    "ayatana-indicator-datetime",
    "ayatana-indicator-sound",
    "ayatana-indicator-notifications",
];

/// `ComplianceCheck.MATE_AUR_PACKAGES` — core + appmenu.
pub const MATE_AUR_PACKAGES: &[&str] = &[
    "mate-tweak", "brisk-menu",
    "mate-indicator-applet",
    // Не общий `vala-panel-appmenu` — он тянет budgie-desktop и vala-panel.
    "vala-panel-appmenu-mate",
    "vala-panel-appmenu-registrar",
];

/// Пакеты, которые починка соответствия ставит штатным путём (`install_pkg`
/// → помощник пакетов): шрифты Ubuntu (MATE), обои и значки COSMIC,
/// дополнения Budgie. Белый список установки помощника
/// (`system_plan::pkg_helper_install_allowlist`) берёт их отсюда; новое имя в
/// `install_pkg` без записи здесь помощник отвергнет — сторожит
/// `debug_assert` в самом `install_pkg`.
pub const FIX_PACKAGES: &[&str] = &[
    "ttf-ubuntu-font-family", "cosmic-wallpapers", "cosmic-icon-theme", "budgie-extras",
];

/// `ComplianceCheck._ICON_FALLBACKS` — по убыванию предпочтения для DE.
pub const ICON_FALLBACKS: &[(&str, &[&str])] = &[
    ("kde", &["breeze-dark", "breeze", "Papirus-Dark", "Adwaita"]),
    ("gnome", &["Adwaita", "Papirus-Dark"]),
    ("cinnamon", &["Mint-Y", "Adwaita", "Papirus-Dark"]),
    ("cinnamon-mint", &["Mint-Y", "Adwaita", "Papirus-Dark"]),
    ("mate", &["mate", "Adwaita", "Papirus-Dark"]),
    ("xfce", &["Adwaita", "Papirus-Dark"]),
    ("budgie", &["Adwaita", "Papirus-Dark"]),
    ("cosmic", &["Cosmic", "Adwaita", "Papirus-Dark"]),
    ("hyprland", &["Papirus-Dark", "Adwaita"]),
];

/// Дефолт `_ICON_FALLBACKS.get(de, ("Adwaita", "Papirus-Dark"))`.
const ICON_FALLBACKS_DEFAULT: &[&str] = &["Adwaita", "Papirus-Dark"];

/// Snap → Arch подстановки в `.layout` (порядок важен) —
/// `_download_mate_layouts.snap_map`.
pub const MATE_LAYOUT_SNAP_MAP: &[(&str, &str)] = &[
    ("firefox_firefox.desktop", "firefox.desktop"),
    ("thunderbird_thunderbird.desktop", "thunderbird.desktop"),
    ("snap-store_snap-store.desktop", "org.gnome.Software.desktop"),
    ("snap-store_ubuntu-mate-welcome.desktop", "org.gnome.Software.desktop"),
    ("software-properties-drivers.desktop", "cachyos-hello.desktop"),
];

/// `_check_hyprland` — пакеты компонентов. Порядок как в Python dict.
const HYPRLAND_COMPONENTS: &[&str] = &[
    "hyprland", "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard",
];

/// Подпись компонента Hyprland для журнала проверки. Контекст — потому что
/// «Terminal» и «Panel» в каталоге уже есть в другом значении (строки
/// сравнения столов).
pub fn hyprland_component_label(pkg: &str) -> String {
    match pkg {
        "hyprland" => trc("Hyprland component", "Compositor"),
        "waybar" => trc("Hyprland component", "Panel"),
        "wofi" => trc("Hyprland component", "Launcher"),
        "kitty" => trc("Hyprland component", "Terminal"),
        "grim" => trc("Hyprland component", "Screenshots"),
        "slurp" => trc("Hyprland component", "Area selection"),
        "wl-clipboard" => trc("Hyprland component", "Clipboard"),
        other => other.to_string(),
    }
}

/// Статус шага, который пропущен (не «ошибка» — просто не наш дистрибутив).
pub const SKIPPED: &str = "skipped";

/// Единственная строка лога о пропуске пакетных шагов на не-Arch —
/// без жаргона, одна на весь прогон проверок.
pub fn pkg_unsupported_note() -> String {
    l2(tr("ℹ️ Checking and installing system packages is only available on \
Arch/CachyOS for now — all other settings were checked"))
}

/// Статус шага, который требует установки: не ошибка и не «дистрибутив не
/// поддержан», а «нужно нажать кнопку в приложении».
pub const NEEDS_ACTION: &str = "needs_action";

/// Единственная строка лога об отложенном лечении при входе в систему.
/// Надпись кнопки — та же строка ядра, что написана на самой кнопке.
pub fn privileged_deferred_note() -> String {
    let button = crate::qt_settings::text("compl.button", &[]).unwrap_or_default();
    l2(trf(
        // TRANSLATORS: %1 is the label of the “Check and Install Missing” button.
        "ℹ️ Some parts are missing — Rexon will install them when you click \
“%1” in Settings; we don't ask for a password at login",
        &[&button],
    ))
}

/// Строка журнала с отступом шага (два пробела).
fn l2(s: String) -> String {
    format!("  {s}")
}

/// Строка журнала с отступом подробности шага (четыре пробела).
fn l4(s: String) -> String {
    format!("    {s}")
}

// ============================================================================
// Трейт исполнителя
// ============================================================================

/// Результат `safe_run(..., capture_output=True)`. `ok` = returncode == 0.
/// `None` у [`ComplianceIo::run`] — safe_run вернул None (таймаут/исключение).
#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub ok: bool,
    pub stdout: String,
}

/// Запись `os.scandir`: имя + `entry.is_dir()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Интерфейс исполнителя: каждый метод — ровно один побочный вызов или
/// наблюдение Python-оригинала, в порядке исполнения. В parity-тестах —
/// реплей журнала; в живом мосте — реальный I/O.
pub trait ComplianceIo {
    /// `safe_run(argv, capture_output=True[, timeout=t])`; None — safe_run
    /// вернул None.
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<RunResult>;
    /// `shutil.which(name) is not None`.
    fn which(&mut self, name: &str) -> bool;
    /// `os.environ.get(name, "")`.
    fn env(&mut self, name: &str) -> String;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&mut self, path: &str) -> bool;
    /// `open(path, "r", encoding="utf-8"[, errors=...]).read()`; None — OSError.
    fn read_text(&mut self, path: &str) -> Option<String>;
    /// `os.scandir(path)` → записи; None — OSError.
    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>>;
    /// `os.listdir(path)`.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str);
    /// `open(path, "w").write(content)`.
    fn write_text(&mut self, path: &str, content: &str);
    /// `os.replace(src, dst)`.
    fn replace(&mut self, src: &str, dst: &str);
    /// `os.path.realpath(path)`.
    fn realpath(&mut self, path: &str) -> String;
    /// `tempfile.mkdtemp(prefix=prefix[, dir=dir])` → путь.
    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String;
    /// `tempfile.mkstemp(prefix=prefix, suffix=suffix)` → путь (fd закрыт
    /// исполнителем).
    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String;
    /// `os.chmod(path, mode)`.
    fn chmod(&mut self, path: &str, mode: u32);
    /// `os.remove(path)`.
    fn remove_file(&mut self, path: &str);
    /// `shutil.rmtree(path, ignore_errors=True)`.
    fn remove_tree(&mut self, path: &str);
    /// `shutil.copy2(src, dst)`.
    fn copy_file(&mut self, src: &str, dst: &str);
    /// Шаг от root: операция `op` через сеанс прав окна
    /// ([`crate::privilege::run`]; `files` — пути файлов операции), иначе
    /// прежняя команда `argv`. По умолчанию — прежняя команда: проверки без
    /// окна (вход в систему) и подделки живут прежним путём.
    fn run_priv(
        &mut self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        argv: &[String],
        timeout: Option<u64>,
    ) -> Option<RunResult> {
        let _ = (op, files);
        self.run(argv, timeout)
    }
}

/// Упорядоченные результаты проверок `{check: status}` (порядок вставки =
/// порядок Python dict).
pub type Results = Vec<(String, String)>;

fn put(results: &mut Results, key: &str, status: &str) {
    // Python dict: повторный ключ обновляет значение, порядок сохраняется.
    if let Some(e) = results.iter_mut().find(|(k, _)| k == key) {
        e.1 = status.to_string();
    } else {
        results.push((key.to_string(), status.to_string()));
    }
}

// ============================================================================
// Чистые хелперы
// ============================================================================

/// `os.path.expanduser` для наших входов (`~` / `~/…`) — единый расширитель
/// ядра: ветка `~/.config/…` уважает `$XDG_CONFIG_HOME`.
fn expanduser(home: &str, xdg_config_home: Option<&str>, p: &str) -> String {
    crate::utils::expand_user(p, home, xdg_config_home)
}

/// `str.strip(chars)` — снять с обоих концов любые символы из набора.
fn strip_chars(s: &str, chars: &str) -> String {
    s.trim_matches(|c| chars.contains(c)).to_string()
}

/// `_ICON_DIRS` для данного HOME.
fn icon_dirs(home: &str) -> [String; 3] {
    [
        expanduser(home, None, "~/.local/share/icons"),
        expanduser(home, None, "~/.icons"),
        "/usr/share/icons".to_string(),
    ]
}

/// `detect_current_de` — по значению `XDG_CURRENT_DESKTOP`.
///
/// Своей таблицы окружений здесь больше нет: она одна на весь продукт —
/// [`crate::system::detect_current_de`]. Заодно закрыта дыра, из-за которой
/// проверки соответствия не доставались тайловым: sway, niri и LXQt в
/// прежней таблице отсутствовали, и функция возвращала `None`.
pub fn detect_current_de(xdg_current_desktop: &str) -> Option<String> {
    crate::system::detect_current_de(xdg_current_desktop, "", false)
}

/// `_pkg_install_arch` — argv установки одного пакета на Arch.
///
/// Рукописного отката здесь больше нет. Он повторял `pkexec` + таблицу
/// [`crate::system_plan::install_cmd`] слово в слово, а такая копия живёт
/// ровно до первой правки таблицы: в соседнем месте того же класса
/// (`wallpaper_engine`) копия уже разошлась — потеряла `--needed`. Ответ
/// целиком отдаёт движок ([`crate::system_plan::install_argv`]), он же решает
/// «именованный путь org.rexon.pkg или сырой pkexec».
///
/// Arch здесь не сужение движка, а предмет проверок соответствия: пути
/// починки в этом модуле опираются на pacman и AUR-помощников и на прочих
/// семействах не запускаются вовсе ([`Compliance::pkg_steps_supported`]).
///
/// `None` — ставить нечего (пустое имя пакета).
pub fn pkg_install_arch(pkg: &str, pkg_helper_available: bool) -> Option<Vec<String>> {
    if pkg.is_empty() {
        return None;
    }
    crate::system_plan::install_argv(PkgFamily::Arch, &[pkg.to_string()], pkg_helper_available)
}

/// Помощники AUR в порядке предпочтения. Одна точка на модуль и на
/// исполнитель доустановки (`system_de_exec`), который по этим именам узнаёт
/// команду починки среди прочих запусков проверки.
pub const AUR_HELPERS: &[&str] = &["yay", "paru", "trizen", "pikaur"];

/// argv установки AUR-пакетов — `_install_aur_packages`.
fn aur_install_argv(helper: &str, packages: &[String]) -> Vec<String> {
    crate::aur_install::install_argv(helper, packages, false)
}

/// Валидна ли строка `Directories=` (не пустая) в `.tex`-подобном index.theme.
fn line_has_directories(s: &str) -> bool {
    s.starts_with("Directories=") && s != "Directories="
}

/// `_patch_settings_ini` — чистая генерация нового содержимого settings.ini.
/// `existing` — текущее содержимое файла (None — файла нет / OSError).
pub fn compute_settings_ini(existing: Option<&str>, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = match existing {
        Some(c) => split_lines(c),
        None => Vec::new(),
    };
    let mut has_section = false;
    let mut replaced = false;
    for line in lines.iter_mut() {
        let s = line.trim();
        if s == "[Settings]" {
            has_section = true;
        } else if has_section && s.starts_with(&format!("{key}=")) {
            *line = format!("{key}={value}");
            replaced = true;
            break;
        }
    }
    if !has_section {
        lines.insert(0, "[Settings]".to_string());
    }
    if !replaced {
        let insert_at = if lines.first().map(|l| l.trim()) == Some("[Settings]") {
            1
        } else {
            0
        };
        let idx = (insert_at + 1).min(lines.len());
        lines.insert(idx, format!("{key}={value}"));
    }
    format!("{}\n", lines.join("\n"))
}

/// `str.splitlines()` — по `\n` без завершающего пустого элемента.
fn split_lines(s: &str) -> Vec<String> {
    // Python splitlines(): без хвостового пустого сегмента.
    let mut out: Vec<String> = s.split('\n').map(|l| l.to_string()).collect();
    if s.ends_with('\n') {
        out.pop();
    }
    if out.len() == 1 && out[0].is_empty() && s.is_empty() {
        out.clear();
    }
    out
}

/// Применить snap→Arch подстановки к тексту layout'а — `_download_mate_layouts`.
pub fn apply_snap_map(text: &str) -> String {
    let mut txt = text.to_string();
    for (snap_n, arch_n) in MATE_LAYOUT_SNAP_MAP {
        txt = txt.replace(
            &format!("/var/lib/snapd/desktop/applications/{snap_n}"),
            &format!("/usr/share/applications/{arch_n}"),
        );
    }
    txt
}

/// `shlex.quote` — минимальное POSIX-экранирование (как в CPython).
fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '%' | '_' | '-' | '+' | '=' | ':' | ',' | '.' | '/'));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// Fallback-скрипт копирования layout'ов (сырой pkexec-путь) —
/// `_download_mate_layouts`. `staged_files` — `os.listdir(staged)`.
pub fn mate_layouts_bash_script(
    staged: &str,
    staged_files: &[String],
    layouts_dst: &str,
) -> String {
    let qdst = shlex_quote(layouts_dst);
    let mut slines: Vec<String> = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("install -d {qdst}"),
    ];
    for f in staged_files {
        let src = shlex_quote(&join(staged, f));
        let dst = shlex_quote(&format!("{layouts_dst}/"));
        slines.push(format!("cp -f {src} {dst}"));
    }
    slines.push(format!("chmod -R a+rX {qdst}"));
    format!("{}\n", slines.join("\n"))
}

/// `os.path.join` (двух-аргументный POSIX).
fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.basename`.
fn basename(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// `os.path.dirname`.
fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

// ============================================================================
// Основной класс проверок
// ============================================================================

/// Состояние пакета для проверок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgState {
    Installed,
    Missing,
    /// Проверка не проводилась: семейство пакетного менеджера не Arch, а
    /// имена пакетов в проверках арчевые.
    Unsupported,
}

/// Чем кончилась попытка починки (установка пакета, AUR, доустановка
/// раскладок).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixOutcome {
    /// Починили.
    Done,
    /// Пытались и не смогли.
    Failed,
    /// Не пытались: идёт автозапуск, пароль и сеть при входе запрещены.
    Deferred,
}

impl FixOutcome {
    /// Статус шага для таблицы результатов.
    pub fn status(self) -> &'static str {
        match self {
            FixOutcome::Done => "fixed",
            FixOutcome::Failed => "failed",
            FixOutcome::Deferred => NEEDS_ACTION,
        }
    }
}

/// Есть ли у DE набор проверок (`ComplianceCheck._DE_CHECKS`) — свободная
/// функция, чтобы спрашивать без построения контекста проверок.
pub fn has_checks(de_id: &str) -> bool {
    matches!(
        de_id,
        "mate" | "kde" | "gnome" | "cosmic" | "xfce" | "cinnamon-mint" | "budgie"
            | "hyprland"
    )
}

/// Что именно доустанавливается по кнопке «Проверить и доустановить» —
/// подпись для человека по ключу проверки.
///
/// Ключи — ровно те, которым проверка умеет дать [`NEEDS_ACTION`] (всё, что
/// лечится паролем администратора или загрузкой из сети). Окно вопроса
/// перечисляет недостающее этими словами, а не именами ключей: покупатель не
/// должен читать `mate_aur_pkgs`. Полноту таблицы держит сверка
/// `system_de_exec::tests::every_deferred_fix_has_a_label` — она прогоняет
/// проверки всех столов на «пустой» машине и требует подпись для КАЖДОГО
/// отложенного ключа, так что новая починка без подписи покраснеет сразу.
///
/// Подписи — исходные (английские) строки каталога, помеченные `n_`;
/// переводит их [`fix_label`].
pub const FIX_LABELS: &[(&str, &str)] = &[
    ("mate_layouts", n_("MATE panel layouts (downloaded from the internet)")),
    ("mate_yaru_theme", n_("Yaru theme for MATE (from the AUR)")),
    ("mate_ubuntu_fonts", n_("Ubuntu fonts")),
    ("mate_aur_pkgs", n_("MATE panel add-ons and system tray icons (from the AUR)")),
    ("cosmic_wallpapers", n_("COSMIC wallpapers")),
    ("cosmic_icons", n_("COSMIC icons")),
    ("cinnamon_themes", n_("Linux Mint themes (from the AUR)")),
    ("budgie_extras", n_("Budgie add-ons")),
];

/// Подпись недостающего по ключу проверки (на языке интерфейса); `None` —
/// ключ не из таблицы.
pub fn fix_label(key: &str) -> Option<String> {
    FIX_LABELS.iter().find(|(k, _)| *k == key).map(|(_, v)| tr(v))
}

/// Что осталось доустановить по итогам проверки: ключи со статусом
/// [`NEEDS_ACTION`] в порядке проверок и их подписи. Ключ без подписи не
/// теряется — показывается как есть (сверка не даст такому появиться).
pub fn deferred_fixes(results: &[(String, String)]) -> Vec<(String, String)> {
    results
        .iter()
        .filter(|(_, status)| status == NEEDS_ACTION)
        .map(|(key, _)| (key.clone(), fix_label(key).unwrap_or_else(|| key.clone())))
        .collect()
}

/// `run_for_current_de`: текущий DE не опознан по `XDG_CURRENT_DESKTOP`.
pub fn msg_unknown_de() -> String {
    tr("⚠️ Couldn't detect the current desktop")
}

/// Шапка одного DE в общем прогоне (`run_for_installed_des`): рамка из `═`,
/// строка с именем DE и снова рамка. Единая точка (правило #12) — её же берёт
/// мост для Python-цикла, чтобы лог обоих путей не разъезжался.
pub fn de_banner_lines(de_id: &str) -> Vec<String> {
    let sep = "═".repeat(50);
    vec![format!("\n{sep}"), l2(trf("Desktop: %1", &[&de_id])), sep]
}

/// Контекст проверок: HOME + семейство пакетного менеджера + разрешение на
/// привилегированные действия + наличие привилегированных helper'ов
/// (подаются параметром — `family`, `allow_privileged`,
/// `_pkg_helper_available` / `_stage_helper_available` живого Python).
pub struct Compliance<'a, IO: ComplianceIo> {
    pub io: &'a mut IO,
    pub home: String,
    /// `$XDG_CONFIG_HOME` (пусто/None — стандартный `~/.config`).
    pub xdg_config_home: Option<String>,
    /// Семейство пакетного менеджера текущего дистрибутива.
    pub pkg_family: PkgFamily,
    /// Можно ли лечить: ставить пакеты, звать AUR-хелпер, клонировать
    /// репозиторий, копировать в системные каталоги через `pkexec`.
    /// `false` — автозапуск при входе в систему: только диагностика.
    pub allow_privileged: bool,
    pub pkg_helper_available: bool,
    pub stage_helper_available: bool,
    /// Строка о пропуске пакетных шагов уже выдана в этом прогоне.
    pkg_note_done: bool,
    /// Строка об отложенном лечении уже выдана в этом прогоне.
    deferred_note_done: bool,
}

impl<'a, IO: ComplianceIo> Compliance<'a, IO> {
    pub fn new(
        io: &'a mut IO,
        home: &str,
        pkg_family: PkgFamily,
        allow_privileged: bool,
        pkg_helper_available: bool,
        stage_helper_available: bool,
    ) -> Self {
        Self::with_xdg(
            io,
            home,
            None,
            pkg_family,
            allow_privileged,
            pkg_helper_available,
            stage_helper_available,
        )
    }

    /// То же, но с явным `$XDG_CONFIG_HOME` (живой вызов из Python).
    #[allow(clippy::too_many_arguments)]
    pub fn with_xdg(
        io: &'a mut IO,
        home: &str,
        xdg_config_home: Option<&str>,
        pkg_family: PkgFamily,
        allow_privileged: bool,
        pkg_helper_available: bool,
        stage_helper_available: bool,
    ) -> Self {
        Compliance {
            io,
            home: home.to_string(),
            xdg_config_home: xdg_config_home.map(str::to_string),
            pkg_family,
            allow_privileged,
            pkg_helper_available,
            stage_helper_available,
            pkg_note_done: false,
            deferred_note_done: false,
        }
    }

    fn expand(&self, p: &str) -> String {
        expanduser(&self.home, self.xdg_config_home.as_deref(), p)
    }

    // ── Гейт по семейству пакетного менеджера ───────────────────────────

    /// Можно ли выполнять пакетные шаги: имена пакетов и команды установки
    /// арчевые, поэтому только на Arch/CachyOS (и производных).
    pub fn pkg_steps_supported(&self) -> bool {
        self.pkg_family == PkgFamily::Arch
    }

    /// Одна понятная строка на весь прогон о пропуске пакетных шагов.
    fn note_pkg_unsupported(&mut self, log: &mut Vec<String>) {
        if !self.pkg_note_done {
            self.pkg_note_done = true;
            log.push(pkg_unsupported_note());
        }
    }

    /// Пропустить пакетный шаг: пометка (однократно) + статус `skipped`
    /// каждому ключу шага. Общая точка для всех DE.
    fn skip_pkg_step<S: AsRef<str>>(
        &mut self,
        results: &mut Results,
        keys: &[S],
        log: &mut Vec<String>,
    ) {
        self.note_pkg_unsupported(log);
        for k in keys {
            put(results, k.as_ref(), SKIPPED);
        }
    }

    // ── Гейт привилегированных действий (автозапуск при входе) ──────────

    /// Одна понятная строка на весь прогон об отложенном лечении.
    fn note_deferred(&mut self, log: &mut Vec<String>) {
        if !self.deferred_note_done {
            self.deferred_note_done = true;
            log.push(privileged_deferred_note());
        }
    }

    /// Можно ли сейчас лечить (ставить/копировать под root, ходить в сеть).
    /// Если нельзя — помечает прогон и возвращает `false`: вызывающий отдаёт
    /// [`FixOutcome::Deferred`]. Единая точка для всех видов починки.
    fn may_fix(&mut self, log: &mut Vec<String>) -> bool {
        if self.allow_privileged {
            return true;
        }
        self.note_deferred(log);
        false
    }

    // ── Утилиты проверки ────────────────────────────────────────────────

    /// Состояние пакета. На не-Arch команда НЕ запускается вовсе — единая
    /// точка гейта для всех проверок (имена пакетов в них арчевые).
    ///
    /// Команду даёт общий движок [`crate::system_plan::query_cmd`] — тот же,
    /// которым спрашивают `PackageManager.is_installed` и проверка
    /// пакета-поставщика темы Plymouth; решение — общее
    /// [`crate::system_plan::pkg_installed_from_run`]. Раньше здесь стоял
    /// зашитый `pacman -Qi <pkg>` со своим разбором исхода: третья копия
    /// одного и того же вопроса. Ответ от замены не меняется — `pacman -Q`
    /// и `pacman -Qi` дают одинаковый код возврата и на установленном
    /// пакете, и на отсутствующем; вывод команды здесь не читается вовсе.
    ///
    /// `Err` от движка (семейство UNKNOWN) недостижим за гейтом, но
    /// трактуется как «проверка недоступна», а не как «не установлен»:
    /// выдумывать отсутствие пакета из-за незнакомого дистрибутива нельзя —
    /// это привело бы к установке под pkexec прямо при входе в систему.
    pub fn pkg_state(&mut self, pkg: &str) -> PkgState {
        if !self.pkg_steps_supported() {
            return PkgState::Unsupported;
        }
        let Ok(argv) = crate::system_plan::query_cmd(self.pkg_family, pkg) else {
            return PkgState::Unsupported;
        };
        let ok = self.io.run(&argv, None).map(|r| r.ok);
        if crate::system_plan::pkg_installed_from_run(ok) {
            PkgState::Installed
        } else {
            PkgState::Missing
        }
    }

    fn is_pkg_installed(&mut self, pkg: &str) -> bool {
        self.pkg_state(pkg) == PkgState::Installed
    }

    /// Отсутствующие пакеты из списка; `None` — проверка недоступна
    /// (не-Arch).
    fn missing_pkgs(&mut self, pkgs: &[&str]) -> Option<Vec<String>> {
        if !self.pkg_steps_supported() {
            return None;
        }
        Some(
            pkgs.iter()
                .filter(|p| !self.is_pkg_installed(p))
                .map(|p| p.to_string())
                .collect(),
        )
    }

    fn find_aur_helper(&mut self) -> Option<&'static str> {
        AUR_HELPERS
            .iter()
            .copied()
            .find(|h| self.io.which(h))
    }

    /// Установка одного пакета штатным путём Arch — единая точка вместо
    /// россыпи `pkg_install_arch` + `io.run` по проверкам.
    fn install_pkg(&mut self, pkg: &str, log: &mut Vec<String>) -> FixOutcome {
        if !self.pkg_steps_supported() {
            self.note_pkg_unsupported(log);
            return FixOutcome::Deferred;
        }
        if !self.may_fix(log) {
            return FixOutcome::Deferred;
        }
        debug_assert!(FIX_PACKAGES.contains(&pkg), "{pkg} не в FIX_PACKAGES");
        // Пустое имя пакета — дефект вызывающего, а не «нечего делать»:
        // команды нет, запускать нечего, чинить не удалось.
        let Some(argv) = pkg_install_arch(pkg, self.pkg_helper_available) else {
            return FixOutcome::Failed;
        };
        // Таблица починки — PkgSource::DesktopRepair (FIX_PACKAGES).
        let op = crate::privilege::op::PrivOp::PkgInstall {
            family: PkgFamily::Arch,
            packages: vec![pkg.to_string()],
            source: crate::privilege::op::PkgSource::DesktopRepair,
        };
        if matches!(self.io.run_priv(&op, &[], &argv, None), Some(r) if r.ok) {
            FixOutcome::Done
        } else {
            FixOutcome::Failed
        }
    }

    fn install_aur_packages(&mut self, packages: &[String], log: &mut Vec<String>) -> FixOutcome {
        if !self.pkg_steps_supported() {
            // Защита от дурака: AUR-хелпер не существует вне Arch.
            self.note_pkg_unsupported(log);
            return FixOutcome::Deferred;
        }
        if !self.may_fix(log) {
            return FixOutcome::Deferred;
        }
        let helper = match self.find_aur_helper() {
            Some(h) => h,
            None => {
                log.push(l4(tr("⚠️ yay/paru not found — install the packages manually")));
                return FixOutcome::Failed;
            }
        };
        log.push(l4(trf("📥 Installing with %1...", &[&helper])));
        let argv = aur_install_argv(helper, packages);
        if let Some(r) = self.io.run(&argv, None) {
            if r.ok {
                log.push(l4(trf("✅ Installed: %1", &[&packages.join(", ")])));
                return FixOutcome::Done;
            }
        }
        log.push(l4(tr("⚠️ Couldn't install (try installing manually)")));
        FixOutcome::Failed
    }

    fn gsettings_get(&mut self, schema: &str, key: &str) -> Option<String> {
        let argv = vec![
            "gsettings".to_string(),
            "get".to_string(),
            schema.to_string(),
            key.to_string(),
        ];
        match self.io.run(&argv, None) {
            Some(r) if r.ok => Some(r.stdout.trim().to_string()),
            _ => None,
        }
    }

    fn gsettings_set(&mut self, schema: &str, key: &str, value: &str) -> bool {
        let argv = vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            key.to_string(),
            value.to_string(),
        ];
        matches!(self.io.run(&argv, None), Some(r) if r.ok)
    }

    // ── Валидность icon-темы ────────────────────────────────────────────

    fn icon_theme_index_path(&mut self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        for base in icon_dirs(&self.home) {
            let p = join(&join(&base, name), "index.theme");
            if self.io.is_file(&p) {
                return Some(p);
            }
        }
        None
    }

    fn is_valid_icon_theme(&mut self, name: &str) -> bool {
        let path = match self.icon_theme_index_path(name) {
            Some(p) => p,
            None => return false,
        };
        let content = match self.io.read_text(&path) {
            Some(c) => c,
            None => return false,
        };
        for line in content.split('\n') {
            let s = line.trim();
            if line_has_directories(s) {
                return true;
            }
        }
        false
    }

    fn pick_fallback_icon_theme(&mut self, de: &str) -> Option<String> {
        let names: Vec<String> = ICON_FALLBACKS
            .iter()
            .find(|(k, _)| *k == de)
            .map(|(_, v)| *v)
            .unwrap_or(ICON_FALLBACKS_DEFAULT)
            .iter()
            .map(|s| s.to_string())
            .collect();
        for name in names {
            if self.is_valid_icon_theme(&name) {
                return Some(name);
            }
        }
        // Последний шанс — любая установленная валидная тема.
        for base in icon_dirs(&self.home) {
            if !self.io.is_dir(&base) {
                continue;
            }
            let entries = match self.io.scandir(&base) {
                Some(e) => e,
                None => continue,
            };
            for entry in entries {
                if entry.is_dir && self.is_valid_icon_theme(&entry.name) {
                    return Some(entry.name);
                }
            }
        }
        None
    }

    fn read_current_icon_theme(&mut self, de: &str) -> Option<String> {
        if matches!(de, "gnome" | "budgie" | "cosmic") {
            return self
                .gsettings_get("org.gnome.desktop.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if matches!(de, "cinnamon" | "cinnamon-mint") {
            return self
                .gsettings_get("org.cinnamon.desktop.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if de == "mate" {
            return self
                .gsettings_get("org.mate.interface", "icon-theme")
                .map(|v| strip_chars(&v, "'\""));
        }
        if de == "xfce" {
            let argv = vec![
                "xfconf-query".to_string(),
                "-c".to_string(),
                "xsettings".to_string(),
                "-p".to_string(),
                "/Net/IconThemeName".to_string(),
            ];
            return match self.io.run(&argv, None) {
                Some(r) if r.ok => Some(r.stdout.trim().to_string()),
                _ => None,
            };
        }
        if de == "kde" {
            let argv = vec![
                "kreadconfig6".to_string(),
                "--file".to_string(),
                "kdeglobals".to_string(),
                "--group".to_string(),
                "Icons".to_string(),
                "--key".to_string(),
                "Theme".to_string(),
                "--default".to_string(),
                "".to_string(),
            ];
            return match self.io.run(&argv, None) {
                Some(r) if r.ok => Some(r.stdout.trim().to_string()),
                _ => None,
            };
        }
        None
    }

    fn write_icon_theme(&mut self, de: &str, theme: &str) -> bool {
        let mut ok = false;
        if matches!(de, "gnome" | "budgie" | "cosmic") {
            ok = self.gsettings_set("org.gnome.desktop.interface", "icon-theme", theme);
        } else if matches!(de, "cinnamon" | "cinnamon-mint") {
            ok = self.gsettings_set("org.cinnamon.desktop.interface", "icon-theme", theme);
        } else if de == "mate" {
            ok = self.gsettings_set("org.mate.interface", "icon-theme", theme);
        } else if de == "xfce" {
            let argv = vec![
                "xfconf-query".to_string(),
                "-c".to_string(),
                "xsettings".to_string(),
                "-p".to_string(),
                "/Net/IconThemeName".to_string(),
                "-s".to_string(),
                theme.to_string(),
            ];
            ok = matches!(self.io.run(&argv, None), Some(r) if r.ok);
        } else if de == "kde" {
            let argv = vec![
                "kwriteconfig6".to_string(),
                "--file".to_string(),
                "kdeglobals".to_string(),
                "--group".to_string(),
                "Icons".to_string(),
                "--key".to_string(),
                "Theme".to_string(),
                theme.to_string(),
            ];
            ok = matches!(self.io.run(&argv, None), Some(r) if r.ok);
        }
        // Синк в settings.ini для GTK-приложений.
        for gtk_ver in ["gtk-3.0", "gtk-4.0"] {
            let ini = self.expand(&format!("~/.config/{gtk_ver}/settings.ini"));
            self.patch_settings_ini(&ini, "gtk-icon-theme-name", theme);
        }
        ok
    }

    fn patch_settings_ini(&mut self, path: &str, key: &str, value: &str) {
        self.io.makedirs(&dirname(path));
        let existing = if self.io.is_file(path) {
            self.io.read_text(path)
        } else {
            None
        };
        let content = compute_settings_ini(existing.as_deref(), key, value);
        let tmp = format!("{path}.tmp");
        self.io.write_text(&tmp, &content);
        self.io.replace(&tmp, path);
    }

    fn check_icon_theme_valid(&mut self, de: &str, log: &mut Vec<String>) -> String {
        log.push(l2(tr("🖼️ Checking the icon theme...")));
        let mut sources: Vec<(String, Option<String>)> =
            vec![(de.to_string(), self.read_current_icon_theme(de))];
        if de == "kde" {
            if let Some(gs) = self.gsettings_get("org.gnome.desktop.interface", "icon-theme") {
                let gs = strip_chars(&gs, "'\"");
                if !gs.is_empty() && Some(&gs) != sources[0].1.as_ref() {
                    sources.push(("__gsettings__".to_string(), Some(gs)));
                }
            }
        }

        let mut fixed = false;
        let mut failed = false;
        let mut any_problem = false;
        for (tag, current) in sources.clone() {
            let label = if tag == "kde" {
                "kdeglobals"
            } else if tag == "__gsettings__" {
                "gsettings"
            } else {
                tag.as_str()
            };
            let current = match current {
                Some(c) if !c.is_empty() => c,
                _ => {
                    log.push(l4(trf("ℹ️ %1: no icon theme set", &[&label])));
                    continue;
                }
            };
            if self.is_valid_icon_theme(&current) {
                log.push(l4(trf("✅ %1: %2 — valid", &[&label, &current])));
                continue;
            }
            any_problem = true;
            log.push(l4(trf(
                "⚠️ %1: “%2” is not valid (no Directories= line, or the theme isn't on disk)",
                &[&label, &current],
            )));
            let fallback = match self.pick_fallback_icon_theme(de) {
                Some(f) => f,
                None => {
                    log.push(l4(tr("❌ Couldn't find any valid icon theme")));
                    failed = true;
                    continue;
                }
            };
            let ok_w = if tag == "__gsettings__" {
                self.gsettings_set("org.gnome.desktop.interface", "icon-theme", &fallback)
            } else {
                self.write_icon_theme(de, &fallback)
            };
            if ok_w {
                log.push(format!("    ✅ {label}: {current} → {fallback}"));
                fixed = true;
            } else {
                log.push(l4(trf("❌ %1: couldn't write %2", &[&label, &fallback])));
                failed = true;
            }
        }

        if !any_problem {
            return "ok".to_string();
        }
        if failed {
            return "failed".to_string();
        }
        if fixed { "fixed" } else { "ok" }.to_string()
    }

    // ── MATE ────────────────────────────────────────────────────────────

    fn check_mate(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();
        let lang = self.io.env("LANG");

        // 1. Локализация Центра управления
        if lang.starts_with("ru") {
            log.push(l2(tr("🌐 Checking the Control Center translation...")));
            // Не подпись интерфейса, а ДАННЫЕ: русские названия «Общих
            // задач» Центра управления MATE, которые пишутся в gsettings
            // (проверка идёт только под русской локалью). Не переводить.
            let expected = "['Изменить тему;mate-appearance-properties.desktop', \
                'Предпочтительные приложения;mate-default-applications-properties.desktop', \
                'Информация о системе MATE;mate-system-info.desktop']";
            let current = self.gsettings_get("org.mate.control-center", "cc-actions-list");
            match current {
                Some(cur) => {
                    if cur.contains("Change Theme") || cur.contains("Set Preferred") {
                        self.gsettings_set(
                            "org.mate.control-center",
                            "cc-actions-list",
                            expected,
                        );
                        log.push(l4(tr("✅ Fixed: “Common Tasks” are now translated")));
                        put(&mut results, "mate_cc_locale", "fixed");
                    } else {
                        log.push(l4(tr("✅ Translation is fine")));
                        put(&mut results, "mate_cc_locale", "ok");
                    }
                }
                None => {
                    log.push(l4(trf("⚠️ gsettings is unavailable (not running %1?)", &[&"MATE"])));
                    put(&mut results, "mate_cc_locale", "failed");
                }
            }
        }

        // 2. Терминал по умолчанию
        log.push(l2(tr("🖥️ Checking the default terminal...")));
        let current_term = self.gsettings_get("org.mate.applications-terminal", "exec");
        match current_term {
            Some(ct) => {
                let ct = strip_chars(&ct, "'");
                if ct == "mate-terminal" {
                    log.push(l4(trf("✅ Terminal: %1", &[&"mate-terminal"])));
                    put(&mut results, "mate_terminal", "ok");
                } else if self.io.which("mate-terminal") {
                    self.gsettings_set(
                        "org.mate.applications-terminal",
                        "exec",
                        "mate-terminal",
                    );
                    self.gsettings_set("org.mate.applications-terminal", "exec-arg", "-x");
                    log.push(l4(trf("✅ Fixed: %1 → %2", &[&ct, &"mate-terminal"])));
                    put(&mut results, "mate_terminal", "fixed");
                } else {
                    log.push(l4(trf("ℹ️ Terminal: %1 (%2 isn't installed)", &[&ct, &"mate-terminal"])));
                    put(&mut results, "mate_terminal", "ok");
                }
            }
            None => {
                put(&mut results, "mate_terminal", "failed");
            }
        }

        // 3. Все 8 layout'ов. Доустановка кладёт файлы в системный каталог
        //    через привилегированный helper и подставляет арчевые .desktop —
        //    поэтому шаг тоже под гейтом семейства (иначе окно ввода пароля
        //    при каждом входе в систему на чужом дистрибутиве).
        if self.pkg_steps_supported() {
            log.push(l2(tr("📐 Checking all 8 Ubuntu MATE panel layouts...")));
            let layouts_dir = "/usr/share/mate-panel/layouts";
            let missing_layouts: Vec<&str> = MATE_EXPECTED_LAYOUTS
                .iter()
                .copied()
                .filter(|name| {
                    let p = join(layouts_dir, &format!("{name}.layout"));
                    !self.io.exists(&p)
                })
                .collect();
            if missing_layouts.is_empty() {
                log.push(l4(tr("✅ All 8 layouts are present (Familiar → Traditional)")));
                put(&mut results, "mate_layouts", "ok");
            } else {
                log.push(l4(trf(
                    "⚠️ Missing (%1): %2",
                    &[&missing_layouts.len(), &missing_layouts.join(", ")],
                )));
                if !self.may_fix(log) {
                    // Автозапуск: ни git clone (сеть), ни pkexec (пароль).
                    put(&mut results, "mate_layouts", NEEDS_ACTION);
                } else if self.io.which("git") {
                    log.push(l4(tr("📥 Downloading layouts from ubuntu-mate-settings...")));
                    let ok = self.download_mate_layouts(log);
                    put(&mut results, "mate_layouts", if ok { "fixed" } else { "failed" });
                } else {
                    // Имя пакета — по семейству ЭТОЙ проверки: на Fedora и
                    // openSUSE git зовётся git-core (правило #13).
                    let hint = crate::utils::tool_package_hint("git", self.pkg_family.as_str());
                    log.push(
                        l4(trf("⚠️ git isn't installed — skipping. %1", &[&hint]))
                            .trim_end()
                            .to_string(),
                    );
                    put(&mut results, "mate_layouts", "failed");
                }
            }
        } else {
            self.skip_pkg_step(&mut results, &["mate_layouts"], log);
        }

        // 4. gschema override
        log.push(l2(tr("🎨 Checking the gschema override (Yaru-MATE, Ubuntu fonts)...")));
        let gschema_path =
            "/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override";
        if self.io.exists(gschema_path) {
            log.push(l4(tr("✅ gschema override is installed")));
            put(&mut results, "mate_gschema", "ok");
        } else {
            log.push(l4(tr("⚠️ gschema override is missing")));
            log.push(l4(tr("ℹ️ Reinstall MATE with Rexon to set it up automatically")));
            put(&mut results, "mate_gschema", "failed");
        }

        // 5. Yaru-MATE тема
        // ⛔ Прежние имена `yaru-mate-gtk-theme` / `yaru-mate-icon-theme`
        // НЕ существуют ни в AUR, ни в репозиториях (RPC 2026-09-01,
        // resultcount 0) — установка падала каждый раз.
        let yaru_pkgs = ["ubuntu-mate-themes", "ubuntu-mate-icon-themes"];
        match self.missing_pkgs(&yaru_pkgs) {
            None => self.skip_pkg_step(&mut results, &["mate_yaru_theme"], log),
            Some(yaru_missing) => {
                log.push(l2(tr("🎨 Checking the Yaru-MATE theme...")));
                if yaru_missing.is_empty() {
                    log.push(l4(tr("✅ Yaru-MATE theme is installed")));
                    put(&mut results, "mate_yaru_theme", "ok");
                } else {
                    log.push(l4(trf("⚠️ Not installed: %1", &[&yaru_missing.join(", ")])));
                    let out = self.install_aur_packages(&yaru_missing, log);
                    put(&mut results, "mate_yaru_theme", out.status());
                }
            }
        }

        // 6. Шрифты Ubuntu
        let ubuntu_font_pkg = "ttf-ubuntu-font-family";
        match self.pkg_state(ubuntu_font_pkg) {
            PkgState::Unsupported => {
                self.skip_pkg_step(&mut results, &["mate_ubuntu_fonts"], log)
            }
            PkgState::Installed => {
                log.push(l2(tr("🔤 Checking Ubuntu fonts...")));
                log.push(l4(tr("✅ Ubuntu fonts are installed")));
                put(&mut results, "mate_ubuntu_fonts", "ok");
            }
            PkgState::Missing => {
                log.push(l2(tr("🔤 Checking Ubuntu fonts...")));
                log.push(l4(tr("⚠️ Ubuntu fonts aren't installed")));
                match self.install_pkg(ubuntu_font_pkg, log) {
                    FixOutcome::Done => {
                        log.push(l4(tr("✅ Ubuntu fonts are installed")));
                        put(&mut results, "mate_ubuntu_fonts", "fixed");
                    }
                    FixOutcome::Failed => {
                        log.push(l4(tr("⚠️ Installation failed")));
                        put(&mut results, "mate_ubuntu_fonts", "failed");
                    }
                    FixOutcome::Deferred => {
                        put(&mut results, "mate_ubuntu_fonts", NEEDS_ACTION);
                    }
                }
            }
        }

        // 7. AUR-пакеты MATE + Ayatana
        let mut all_mate_aur: Vec<&str> = MATE_AUR_PACKAGES.to_vec();
        all_mate_aur.extend_from_slice(AYATANA_PACKAGES);
        match self.missing_pkgs(&all_mate_aur) {
            None => self.skip_pkg_step(&mut results, &["mate_aur_pkgs"], log),
            Some(missing_pkgs) => {
                log.push(l2(tr("📦 Checking MATE AUR packages and the Ayatana stack...")));
                if missing_pkgs.is_empty() {
                    log.push(l4(tr(
                        "✅ All AUR packages are installed (including the Ayatana stack)",
                    )));
                    put(&mut results, "mate_aur_pkgs", "ok");
                } else {
                    let head: Vec<String> = missing_pkgs.iter().take(5).cloned().collect();
                    let ellipsis = if missing_pkgs.len() > 5 { "..." } else { "" };
                    log.push(l4(trf(
                        "⚠️ Not installed (%1): %2%3",
                        &[&missing_pkgs.len(), &head.join(", "), &ellipsis],
                    )));
                    let out = self.install_aur_packages(&missing_pkgs, log);
                    put(&mut results, "mate_aur_pkgs", out.status());
                }
            }
        }

        // 8. Курсор Yaru
        log.push(l2(tr("🖱️ Checking the Yaru cursor...")));
        let cursor_val = self.gsettings_get("org.mate.peripherals-mouse", "cursor-theme");
        match cursor_val {
            Some(cv) => {
                let cv = strip_chars(&cv, "'");
                if cv == "Yaru" {
                    log.push(l4(trf("✅ Cursor: %1", &[&"Yaru"])));
                    put(&mut results, "mate_cursor", "ok");
                } else {
                    let yaru_cursor_path = "/usr/share/icons/Yaru/cursors";
                    if self.io.is_dir(yaru_cursor_path) {
                        self.gsettings_set(
                            "org.mate.peripherals-mouse",
                            "cursor-theme",
                            "Yaru",
                        );
                        log.push(l4(trf("✅ Fixed: %1 → %2", &[&cv, &"Yaru"])));
                        put(&mut results, "mate_cursor", "fixed");
                    } else {
                        log.push(l4(trf("ℹ️ Cursor: %1 (%2 isn't installed)", &[&cv, &"Yaru"])));
                        put(&mut results, "mate_cursor", "ok");
                    }
                }
            }
            None => {
                put(&mut results, "mate_cursor", "failed");
            }
        }

        // 9. mate-system-info локализация
        if lang.starts_with("ru") {
            log.push(l2(tr("🌐 Checking the mate-utils translation...")));
            let mo_path = "/usr/share/locale/ru/LC_MESSAGES/mate-utils.mo";
            if self.io.exists(mo_path) {
                log.push(l4(tr("ℹ️ Translation file mate-utils.mo is present")));
                log.push(l4(tr(
                    "ℹ️ Device Name/Memory/Processor stay untranslated — a MATE bug, not Rexon's",
                )));
                put(&mut results, "mate_system_info_locale", "ok");
            } else {
                log.push(l4(tr("⚠️ Translation file mate-utils.mo is missing")));
                put(&mut results, "mate_system_info_locale", "failed");
            }
        }

        results
    }

    fn download_mate_layouts(&mut self, log: &mut Vec<String>) -> bool {
        // Защита от дурака: сюда нельзя попасть при входе в систему —
        // здесь и сеть (git clone), и запись в системный каталог.
        if !self.may_fix(log) {
            return false;
        }
        let tmp_repo = self.io.mkdtemp("rexon_ums_", None);
        let layouts_dst = "/usr/share/mate-panel/layouts";

        // if os.path.isdir(tmp_repo): rmtree — mkdtemp всегда создаёт каталог.
        if self.io.is_dir(&tmp_repo) {
            self.io.remove_tree(&tmp_repo);
        }

        let clone_argv = vec![
            "git".to_string(),
            "clone".to_string(),
            "--depth".to_string(),
            "1".to_string(),
            "https://github.com/ubuntu-mate/ubuntu-mate-settings.git".to_string(),
            tmp_repo.clone(),
        ];
        let r = self.io.run(&clone_argv, Some(120));
        if !matches!(&r, Some(rr) if rr.ok) {
            log.push(l4(tr("⚠️ Couldn't clone the repository")));
            return false;
        }

        let layouts_src = join(
            &join(&join(&join(&tmp_repo, "usr"), "share"), "mate-panel"),
            "layouts",
        );
        if !self.io.is_dir(&layouts_src) {
            log.push(l4(tr("⚠️ Layouts folder not found")));
            return false;
        }

        let files: Vec<String> = self
            .io
            .listdir(&layouts_src)
            .into_iter()
            .filter(|f| f.ends_with(".layout") || f.ends_with(".dock"))
            .collect();
        if files.is_empty() {
            return false;
        }

        let staged = self.io.mkdtemp("rexon_layouts_stage_", Some("/tmp"));
        for f in &files {
            let src = join(&layouts_src, f);
            let dst = join(&staged, basename(f));
            self.io.copy_file(&src, &dst);
            if f.ends_with(".layout") {
                let fp = join(&staged, basename(f));
                if let Some(txt) = self.io.read_text(&fp) {
                    let txt = apply_snap_map(&txt);
                    self.io.write_text(&fp, &txt);
                }
            }
        }

        // named org.rexon.stage, иначе hardened сырой pkexec.
        let staged_is_dir = self.io.is_dir(&staged);
        let staged_real = self.io.realpath(&staged);
        let named = crate::system_plan::privileged_stage_cmd(
            "mate-layouts",
            "rexon",
            &staged,
            staged_is_dir,
            Some(&staged_real),
            self.stage_helper_available,
        );
        let r2 = if let Some(named) = named {
            let op = crate::privilege::op::PrivOp::Stage {
                category: "mate-layouts".to_string(),
                name: "rexon".to_string(),
                source: crate::privilege::op::LocalFile { slot: 0, label: staged.clone() },
            };
            self.io.run_priv(&op, std::slice::from_ref(&staged), &named, Some(120))
        } else {
            let staged_files = self.io.listdir(&staged);
            let script = mate_layouts_bash_script(&staged, &staged_files, layouts_dst);
            let tmp_sh = self.io.mkstemp("rexon_layouts_", ".sh");
            self.io.write_text(&tmp_sh, &script);
            self.io.chmod(&tmp_sh, 0o755);
            let r2 = self.io.run(
                &["pkexec".to_string(), "bash".to_string(), tmp_sh.clone()],
                Some(120),
            );
            self.io.remove_file(&tmp_sh);
            r2
        };

        self.io.remove_tree(&staged);
        if self.io.is_dir(&tmp_repo) {
            self.io.remove_tree(&tmp_repo);
        }

        if matches!(&r2, Some(rr) if rr.ok) {
            let n_layout = files.iter().filter(|f| f.ends_with(".layout")).count();
            log.push(l4(ntr(
                "✅ Copied %n layout (paths adapted)",
                "✅ Copied %n layouts (paths adapted)",
                n_layout as u64,
            )));
            true
        } else {
            log.push(l4(tr("⚠️ Administrator access was declined or copying failed")));
            false
        }
    }

    // ── KDE ─────────────────────────────────────────────────────────────

    fn check_kde(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        let icon = self.check_icon_theme_valid("kde", log);
        put(&mut results, "kde_icon_theme", &icon);

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"KDE"])));
        let kdeglobals = self.expand("~/.config/kdeglobals");
        if self.io.exists(&kdeglobals) {
            let content = self.io.read_text(&kdeglobals).unwrap_or_default();
            if content.contains("TerminalApplication=konsole") {
                log.push(l4(trf("✅ Terminal: %1", &[&"konsole"])));
                put(&mut results, "kde_terminal", "ok");
            } else if self.io.which("konsole") {
                if self.io.which("kwriteconfig6") {
                    self.io.run(
                        &[
                            "kwriteconfig6".to_string(),
                            "--file".to_string(),
                            "kdeglobals".to_string(),
                            "--group".to_string(),
                            "General".to_string(),
                            "--key".to_string(),
                            "TerminalApplication".to_string(),
                            "konsole".to_string(),
                        ],
                        None,
                    );
                    self.io.run(
                        &[
                            "kwriteconfig6".to_string(),
                            "--file".to_string(),
                            "kdeglobals".to_string(),
                            "--group".to_string(),
                            "General".to_string(),
                            "--key".to_string(),
                            "TerminalService".to_string(),
                            "org.kde.konsole.desktop".to_string(),
                        ],
                        None,
                    );
                    log.push(l4(trf("✅ Fixed: terminal → %1", &[&"konsole"])));
                    put(&mut results, "kde_terminal", "fixed");
                } else {
                    log.push(l4(trf("⚠️ %1 not found", &[&"kwriteconfig6"])));
                    put(&mut results, "kde_terminal", "failed");
                }
            } else {
                log.push(l4(trf("ℹ️ %1 isn't installed", &[&"konsole"])));
                put(&mut results, "kde_terminal", "ok");
            }
        } else {
            log.push(l4(trf("ℹ️ %1 not found (first launch?)", &[&"kdeglobals"])));
            put(&mut results, "kde_terminal", "ok");
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("plasma-login-manager") {
                log.push(l4(trf("✅ %1 is installed", &[&"Plasma Login Manager"])));
                put(&mut results, "kde_sddm", "ok");
            } else if self.is_pkg_installed("sddm") {
                log.push(l4(trf("✅ %1 is installed", &[&"SDDM"])));
                put(&mut results, "kde_sddm", "ok");
            } else {
                log.push(l4(tr(
                    "⚠️ No display manager (SDDM / Plasma Login Manager) is installed",
                )));
                put(&mut results, "kde_sddm", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["kde_sddm"], log);
        }

        results
    }

    // ── GNOME ───────────────────────────────────────────────────────────

    fn check_gnome(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        let icon = self.check_icon_theme_valid("gnome", log);
        put(&mut results, "gnome_icon_theme", &icon);

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"GNOME"])));
        let term_val = self.gsettings_get(
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        );
        match term_val {
            Some(tv) => {
                let tv = strip_chars(&tv, "'");
                let gnome_terms = ["gnome-terminal", "kgx", "org.gnome.Console"];
                if gnome_terms.contains(&tv.as_str()) {
                    log.push(l4(trf("✅ Terminal: %1", &[&tv])));
                    put(&mut results, "gnome_terminal", "ok");
                } else {
                    let mut done = false;
                    for t in gnome_terms {
                        if self.io.which(t) {
                            self.gsettings_set(
                                "org.gnome.desktop.default-applications.terminal",
                                "exec",
                                t,
                            );
                            log.push(l4(trf("✅ Fixed: %1 → %2", &[&tv, &t])));
                            put(&mut results, "gnome_terminal", "fixed");
                            done = true;
                            break;
                        }
                    }
                    if !done {
                        log.push(l4(trf("ℹ️ Terminal: %1", &[&tv])));
                        put(&mut results, "gnome_terminal", "ok");
                    }
                }
            }
            None => {
                log.push(l4(trf("⚠️ gsettings is unavailable (not running %1?)", &[&"GNOME"])));
                put(&mut results, "gnome_terminal", "failed");
            }
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("gdm") {
                log.push(l4(trf("✅ %1 is installed", &[&"GDM"])));
                put(&mut results, "gnome_gdm", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"GDM"])));
                put(&mut results, "gnome_gdm", "failed");
            }

            log.push(l2(trf("🔧 Checking %1...", &[&"gnome-tweaks"])));
            if self.is_pkg_installed("gnome-tweaks") {
                log.push(l4(trf("✅ %1 is installed", &[&"gnome-tweaks"])));
                put(&mut results, "gnome_tweaks", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"gnome-tweaks"])));
                put(&mut results, "gnome_tweaks", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["gnome_gdm", "gnome_tweaks"], log);
        }

        results
    }

    // ── COSMIC ──────────────────────────────────────────────────────────

    fn check_cosmic(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        // Весь набор проверок COSMIC — пакетный (обои/иконки/keyring).
        if !self.pkg_steps_supported() {
            self.skip_pkg_step(
                &mut results,
                &["cosmic_wallpapers", "cosmic_icons", "cosmic_keyring"],
                log,
            );
            return results;
        }

        log.push(l2(tr("🖼️ Checking COSMIC wallpapers...")));
        if self.is_pkg_installed("cosmic-wallpapers") {
            log.push(l4(trf("✅ %1 is installed", &[&"cosmic-wallpapers"])));
            put(&mut results, "cosmic_wallpapers", "ok");
        } else {
            log.push(l4(if self.allow_privileged {
                trf("⚠️ %1 isn't installed — installing...", &[&"cosmic-wallpapers"])
            } else {
                trf("⚠️ %1 isn't installed", &[&"cosmic-wallpapers"])
            }));
            match self.install_pkg("cosmic-wallpapers", log) {
                FixOutcome::Done => {
                    log.push(l4(trf("✅ %1 is installed", &[&"cosmic-wallpapers"])));
                    put(&mut results, "cosmic_wallpapers", "fixed");
                }
                FixOutcome::Failed => {
                    log.push(l4(tr("⚠️ Installation failed")));
                    put(&mut results, "cosmic_wallpapers", "failed");
                }
                FixOutcome::Deferred => {
                    put(&mut results, "cosmic_wallpapers", NEEDS_ACTION);
                }
            }
        }

        log.push(l2(tr("🎨 Checking COSMIC icons...")));
        if self.is_pkg_installed("cosmic-icon-theme") {
            log.push(l4(trf("✅ %1 is installed", &[&"cosmic-icon-theme"])));
            put(&mut results, "cosmic_icons", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"cosmic-icon-theme"])));
            match self.install_pkg("cosmic-icon-theme", log) {
                FixOutcome::Done => {
                    log.push(l4(trf("✅ %1 is installed", &[&"cosmic-icon-theme"])));
                    put(&mut results, "cosmic_icons", "fixed");
                }
                FixOutcome::Failed => put(&mut results, "cosmic_icons", "failed"),
                FixOutcome::Deferred => put(&mut results, "cosmic_icons", NEEDS_ACTION),
            }
        }

        log.push(l2(trf("🔐 Checking %1...", &[&"gnome-keyring"])));
        if self.is_pkg_installed("gnome-keyring") {
            log.push(l4(trf("✅ %1 is installed", &[&"gnome-keyring"])));
            put(&mut results, "cosmic_keyring", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"gnome-keyring"])));
            put(&mut results, "cosmic_keyring", "failed");
        }

        results
    }

    // ── XFCE ────────────────────────────────────────────────────────────

    fn check_xfce(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        log.push(l2(tr("🖥️ Checking XFCE helpers.rc...")));
        let helpers_path = self.expand("~/.config/xfce4/helpers.rc");
        if self.io.exists(&helpers_path) {
            let content = self.io.read_text(&helpers_path).unwrap_or_default();
            if content.contains("TerminalEmulator=") {
                log.push(l4(tr("✅ helpers.rc is configured")));
                put(&mut results, "xfce_helpers", "ok");
            } else {
                log.push(l4(tr("ℹ️ TerminalEmulator isn't set")));
                put(&mut results, "xfce_helpers", "ok");
            }
        } else {
            log.push(l4(tr("ℹ️ helpers.rc doesn't exist (first launch?)")));
            put(&mut results, "xfce_helpers", "ok");
        }

        if self.pkg_steps_supported() {
            log.push(l2(tr("🔑 Checking the display manager...")));
            if self.is_pkg_installed("lightdm") {
                log.push(l4(trf("✅ %1 is installed", &[&"LightDM"])));
                put(&mut results, "xfce_lightdm", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"LightDM"])));
                put(&mut results, "xfce_lightdm", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["xfce_lightdm"], log);
        }

        results
    }

    // ── Cinnamon (Mint) ─────────────────────────────────────────────────

    fn check_cinnamon(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        // Весь набор проверок Cinnamon — пакетный (Nemo/темы Mint/LightDM).
        if !self.pkg_steps_supported() {
            self.skip_pkg_step(
                &mut results,
                &["cinnamon_nemo", "cinnamon_themes", "cinnamon_lightdm"],
                log,
            );
            return results;
        }

        log.push(l2(tr("📁 Checking the Nemo file manager...")));
        if self.is_pkg_installed("nemo") {
            log.push(l4(trf("✅ %1 is installed", &[&"Nemo"])));
            put(&mut results, "cinnamon_nemo", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"Nemo"])));
            put(&mut results, "cinnamon_nemo", "failed");
        }

        log.push(l2(tr("🎨 Checking Linux Mint themes...")));
        let mint_theme_pkgs = ["mint-themes", "mint-y-icons"];
        let mint_missing: Vec<String> = mint_theme_pkgs
            .iter()
            .filter(|p| !self.is_pkg_installed(p))
            .map(|p| p.to_string())
            .collect();
        if mint_missing.is_empty() {
            log.push(l4(tr("✅ Mint themes are installed")));
            put(&mut results, "cinnamon_themes", "ok");
        } else {
            log.push(l4(trf("⚠️ Not installed: %1", &[&mint_missing.join(", ")])));
            let out = self.install_aur_packages(&mint_missing, log);
            put(&mut results, "cinnamon_themes", out.status());
        }

        log.push(l2(tr("🔑 Checking the display manager...")));
        if self.is_pkg_installed("lightdm") {
            log.push(l4(trf("✅ %1 is installed", &[&"LightDM"])));
            put(&mut results, "cinnamon_lightdm", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 isn't installed", &[&"LightDM"])));
            put(&mut results, "cinnamon_lightdm", "failed");
        }

        results
    }

    // ── Budgie ──────────────────────────────────────────────────────────

    fn check_budgie(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        log.push(l2(trf("🖥️ Checking the %1 terminal...", &[&"Budgie"])));
        let term_val = self.gsettings_get(
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        );
        match term_val {
            Some(tv) => {
                log.push(l4(trf("✅ Terminal: %1", &[&strip_chars(&tv, "'")])));
                put(&mut results, "budgie_terminal", "ok");
            }
            None => {
                log.push(l4(tr("⚠️ gsettings is unavailable")));
                put(&mut results, "budgie_terminal", "failed");
            }
        }

        if self.pkg_steps_supported() {
            log.push(l2(trf("📦 Checking %1...", &[&"budgie-extras"])));
            if self.is_pkg_installed("budgie-extras") {
                log.push(l4(trf("✅ %1 is installed", &[&"budgie-extras"])));
                put(&mut results, "budgie_extras", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"budgie-extras"])));
                // ⚠️ Раньше он ставился помощником AUR — и на машине без
                // yay/paru починка проваливалась зря: `budgie-extras` лежит
                // в extra, pacman берёт его сам.
                let out = self.install_pkg("budgie-extras", log);
                put(&mut results, "budgie_extras", out.status());
            }

            log.push(l2(trf("📶 Checking %1...", &[&"network-manager-applet"])));
            if self.is_pkg_installed("network-manager-applet") {
                log.push(l4(trf("✅ %1 is installed", &[&"network-manager-applet"])));
                put(&mut results, "budgie_nm_applet", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"network-manager-applet"])));
                put(&mut results, "budgie_nm_applet", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["budgie_extras", "budgie_nm_applet"], log);
        }

        results
    }

    // ── Hyprland ────────────────────────────────────────────────────────

    fn check_hyprland(&mut self, log: &mut Vec<String>) -> Results {
        let mut results: Results = Vec::new();

        if self.pkg_steps_supported() {
            log.push(l2(tr("🧱 Checking Hyprland components...")));
            for pkg in HYPRLAND_COMPONENTS {
                let label = hyprland_component_label(pkg);
                if self.is_pkg_installed(pkg) {
                    log.push(format!("    ✅ {label}: {pkg}"));
                    put(&mut results, &format!("hyprland_{pkg}"), "ok");
                } else {
                    log.push(l4(trf("⚠️ %1: %2 isn't installed", &[&label, pkg])));
                    put(&mut results, &format!("hyprland_{pkg}"), "failed");
                }
            }
        } else {
            let keys: Vec<String> = HYPRLAND_COMPONENTS
                .iter()
                .map(|pkg| format!("hyprland_{pkg}"))
                .collect();
            self.skip_pkg_step(&mut results, &keys, log);
        }

        log.push(l2(tr("📝 Checking the configuration...")));
        let hypr_cfg = self.expand("~/.config/hypr/hyprland.conf");
        if self.io.exists(&hypr_cfg) {
            log.push(l4(trf("✅ %1 exists", &[&"hyprland.conf"])));
            put(&mut results, "hyprland_config", "ok");
        } else {
            log.push(l4(trf("⚠️ %1 not found", &[&"hyprland.conf"])));
            log.push(l4(trf(
                "ℹ️ Create it: %1",
                &[&"cp /usr/share/hyprland/hyprland.conf ~/.config/hypr/"],
            )));
            put(&mut results, "hyprland_config", "failed");
        }

        if self.pkg_steps_supported() {
            log.push(l2(trf("🌐 Checking %1...", &[&"xdg-desktop-portal"])));
            if self.is_pkg_installed("xdg-desktop-portal-hyprland") {
                log.push(l4(trf("✅ %1 is installed", &[&"xdg-desktop-portal-hyprland"])));
                put(&mut results, "hyprland_portal", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"xdg-desktop-portal-hyprland"])));
                put(&mut results, "hyprland_portal", "failed");
            }

            log.push(l2(tr("🔐 Checking the polkit agent...")));
            if self.is_pkg_installed("polkit-gnome") {
                log.push(l4(trf("✅ %1 is installed", &[&"polkit-gnome"])));
                put(&mut results, "hyprland_polkit", "ok");
            } else {
                log.push(l4(trf("⚠️ %1 isn't installed", &[&"polkit-gnome"])));
                put(&mut results, "hyprland_polkit", "failed");
            }
        } else {
            self.skip_pkg_step(&mut results, &["hyprland_portal", "hyprland_polkit"], log);
        }

        results
    }

    // ── Диспетчеризация ─────────────────────────────────────────────────

    /// Есть ли набор проверок для DE (`_DE_CHECKS.get(de_id)` — без запуска).
    pub fn has_handler(de_id: &str) -> bool {
        has_checks(de_id)
    }

    fn dispatch(&mut self, de_id: &str, log: &mut Vec<String>) -> Results {
        match de_id {
            "mate" => self.check_mate(log),
            "kde" => self.check_kde(log),
            "gnome" => self.check_gnome(log),
            "cosmic" => self.check_cosmic(log),
            "xfce" => self.check_xfce(log),
            "cinnamon-mint" => self.check_cinnamon(log),
            "budgie" => self.check_budgie(log),
            "hyprland" => self.check_hyprland(log),
            _ => Vec::new(),
        }
    }

    /// `run_for_de` — запустить все проверки для DE. Возвращает результаты и
    /// пишет лог в `log`.
    pub fn run_for_de(&mut self, de_id: &str, log: &mut Vec<String>) -> Results {
        if !Self::has_handler(de_id) {
            log.push(trf("ℹ️ No checks for this desktop: %1", &[&de_id]));
            return Vec::new();
        }
        log.push(trf("🔍 Checking desktop %1...", &[&de_id]));
        let results = self.dispatch(de_id, log);

        let fixed = results.iter().filter(|(_, v)| v == "fixed").count();
        let failed = results.iter().filter(|(_, v)| v == "failed").count();
        let ok = results.iter().filter(|(_, v)| v == "ok").count();
        let skipped = results.iter().filter(|(_, v)| v == SKIPPED).count();
        let needs = results.iter().filter(|(_, v)| v == NEEDS_ACTION).count();
        let mut tail = String::new();
        if skipped > 0 {
            tail.push_str(&trf(", %1 skipped", &[&skipped]));
        }
        if needs > 0 {
            tail.push_str(&trf(", %1 to install", &[&needs]));
        }
        log.push(trf(
            // TRANSLATORS: %1 checks passed, %2 fixed, %3 failed; %4 is an
            // optional tail such as “, 2 skipped, 1 to install”.
            "✅ Check finished: %1 OK, %2 fixed, %3 failed%4",
            &[&ok, &fixed, &failed, &tail],
        ));
        results
    }

    /// `run_for_current_de` — определить текущий DE и прогнать проверки.
    pub fn run_for_current_de(
        &mut self,
        xdg_current_desktop: &str,
        log: &mut Vec<String>,
    ) -> Results {
        match detect_current_de(xdg_current_desktop) {
            Some(de_id) => self.run_for_de(&de_id, log),
            None => {
                log.push(msg_unknown_de());
                Vec::new()
            }
        }
    }

    /// `run_for_installed_des` — прогнать compliance для всех установленных DE.
    pub fn run_for_installed_des(
        &mut self,
        installed_de_ids: &[String],
        log: &mut Vec<String>,
    ) -> Vec<(String, Results)> {
        let mut all_results: Vec<(String, Results)> = Vec::new();
        for de_id in installed_de_ids {
            if Self::has_handler(de_id) {
                log.extend(de_banner_lines(de_id));
                let results = self.run_for_de(de_id, log);
                all_results.push((de_id.clone(), results));
            }
        }
        all_results
    }
}
