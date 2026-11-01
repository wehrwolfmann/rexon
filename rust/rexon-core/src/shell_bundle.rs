//! Связка «тайловый композитор + оболочка» (Noctalia, DankMaterialShell):
//! что ставить, куда и какую строку вписать, как снять ровно её.
//!
//! Шаг 3 поддержки оболочек (решение владельца 01.10.2026): в карточке
//! Hyprland, niri и sway справа блок «Оболочки». Здесь — решения без
//! побочных действий; исполнитель — `system_de_exec` (тот же путь пакетов и
//! тот же пароль, что у установки столов), подписи — `qt_desktops`.
//!
//! Факты (разведка 01.10.2026, noctalia 5.2, DMS 1.6.2, по исходникам и
//! пакетам):
//!   * ни одна оболочка не приносит своей сессии в экран входа: «композитор
//!     с оболочкой» — это строка автозапуска в конфиге композитора;
//!   * Noctalia в официальных репозиториях Arch (`extra/noctalia`) и Fedora
//!     44+ (`noctalia`); `cachyos/noctalia-shell` — это ПРЕЖНЯЯ v4 на
//!     Quickshell, другой продукт, его не ставим;
//!   * DankMaterialShell в Arch разбит: `dms-shell` требует
//!     `dms-shell-compositor`, а его дают только `dms-shell-niri` и
//!     `dms-shell-hyprland`. Для sway варианта нет — в карточке sway DMS
//!     неактивен с честной подсказкой. На прочих дистрибутивах оба шелла —
//!     только из сторонних хранилищ (COPR, OBS, PPA, оверлей GURU), а их
//!     Rexon сам не подключает;
//!   * строка автозапуска — управляемым блоком общего помощника
//!     `compositor_autostart` (метки, синтаксисы, обратимость бит-в-бит);
//!   * DMS ставит юнит `dms.service` с `WantedBy=graphical-session.target`:
//!     ⛔ `systemctl --user enable dms` поднял бы его и в Plasma. Мы юнит не
//!     трогаем вовсе — запуск идёт строкой в конфиге того композитора, где
//!     человек оболочку выбрал (у DMS это штатный путь «без systemd»).
//!
//! ⭐ ДВА РЕЖИМА ОДНОГО КОМПОЗИТОРА. Основной стол остаётся самим собой:
//! строка автозапуска стоит в конфиге постоянно, но срабатывает, только если
//! лежит отметка `<state>/rexon/shells/<композитор>-<оболочка>`. «Переключиться»
//! у оболочки кладёт отметку и выходит из сеанса; «Переключиться» у самого
//! стола (левая часть карточки) отметки этого композитора снимает. Так у
//! стола и у оболочки — свои входы, хотя запись в экране входа одна.

use serde::Serialize;

use crate::compositor_autostart::{self as ca, Refusal, Syntax, TextEdit};
use crate::i18n::trf;
use crate::system::{DesktopShell, PkgFamily};

/// Композиторы, в карточках которых есть блок «Оболочки», — тот же состав,
/// что секция «Тайловые» (`qt_desktops::TILING_IDS`). Plasma сюда не входит:
/// у Noctalia она «not first-class», у DMS поддержки нет вовсе.
pub const SHELL_WMS: &[&str] = &["hyprland", "niri", "sway"];

/// Оболочки карточки стола; пусто — блока «Оболочки» в карточке нет.
pub fn shells_for(de_id: &str) -> &'static [DesktopShell] {
    if SHELL_WMS.contains(&de_id) {
        DesktopShell::all()
    } else {
        &[]
    }
}

/// Оболочка по идентификатору (`DesktopShell::as_str`).
pub fn shell_by_id(id: &str) -> Option<DesktopShell> {
    DesktopShell::all().iter().copied().find(|s| s.as_str() == id)
}

/// Исполняемый файл оболочки — по нему видно, стоит ли пакет.
pub fn binary(shell: DesktopShell) -> &'static str {
    match shell {
        DesktopShell::Noctalia => "noctalia",
        DesktopShell::DankMaterialShell => "dms",
    }
}

/// Команда запуска в сеансе (их документация: `noctalia`, `dms run`).
pub fn launch_command(shell: DesktopShell) -> &'static str {
    match shell {
        DesktopShell::Noctalia => "noctalia",
        DesktopShell::DankMaterialShell => "dms run",
    }
}

/// Почему оболочку в этой карточке поставить нельзя.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// Карточка не тайлового композитора.
    NotForThisDesktop,
    /// DMS под sway: в репозиториях его ставят только вместе с niri или
    /// Hyprland.
    NotPackagedForWm,
    /// В официальных репозиториях этого дистрибутива оболочки нет.
    NotInRepos,
}

/// Объяснение для человека: почему оболочку здесь не поставить. Одна фраза —
/// её показывают и подсказкой у неактивной кнопки, и отказом исполнителя.
pub fn unavailable_text(reason: Unavailable, shell: DesktopShell, wm_name: &str) -> String {
    match reason {
        Unavailable::NotForThisDesktop => trf(
            // TRANSLATORS: %1 is a desktop shell (Noctalia…), %2 a desktop.
            "%1 can't be added to %2.",
            &[&shell.label(), &wm_name],
        ),
        Unavailable::NotPackagedForWm => trf(
            // TRANSLATORS: %1 is a desktop shell, %2 a compositor (sway).
            "%1 isn't packaged for %2: your distribution ships it only together with niri or Hyprland.",
            &[&shell.label(), &wm_name],
        ),
        Unavailable::NotInRepos => trf(
            // TRANSLATORS: %1 is a desktop shell (Noctalia, DankMaterialShell).
            "%1 isn't in your distribution's official repositories.",
            &[&shell.label()],
        ),
    }
}

/// Сначала нужен сам стол — подсказка у неактивной «Установить» и отказ
/// исполнителя.
pub fn install_desktop_first(wm_name: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Hyprland, niri, sway).
    trf("Install %1 first", &[&wm_name])
}

/// Пакеты оболочки для этого композитора и дистрибутива — порядок установки.
pub fn packages(
    shell: DesktopShell,
    wm: &str,
    family: PkgFamily,
) -> Result<Vec<&'static str>, Unavailable> {
    if !SHELL_WMS.contains(&wm) {
        return Err(Unavailable::NotForThisDesktop);
    }
    match shell {
        DesktopShell::Noctalia => match family {
            PkgFamily::Arch | PkgFamily::Fedora => Ok(vec!["noctalia"]),
            _ => Err(Unavailable::NotInRepos),
        },
        DesktopShell::DankMaterialShell => {
            if family != PkgFamily::Arch {
                return Err(Unavailable::NotInRepos);
            }
            match wm {
                "niri" => Ok(vec!["dms-shell", "dms-shell-niri"]),
                "hyprland" => Ok(vec!["dms-shell", "dms-shell-hyprland"]),
                _ => Err(Unavailable::NotPackagedForWm),
            }
        }
    }
}

/// Все пакеты всех оболочек под все композиторы семейства — белый список
/// установки помощника пакетов (`system_plan::pkg_helper_install_allowlist`).
pub fn all_packages(family: PkgFamily) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for shell in DesktopShell::all() {
        for wm in SHELL_WMS {
            for p in packages(*shell, wm, family).unwrap_or_default() {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Пакеты, которые «Убрать» снимает у этого композитора.
///
/// `still_used_by` — другие композиторы, в конфиге которых строка этой же
/// оболочки остаётся: тогда общие пакеты (`noctalia`, `dms-shell`) живут
/// дальше, а уходит только своё для композитора (`dms-shell-niri`).
pub fn packages_to_remove(
    shell: DesktopShell,
    wm: &str,
    family: PkgFamily,
    still_used_by: &[&str],
) -> Vec<&'static str> {
    let Ok(own) = packages(shell, wm, family) else {
        return Vec::new();
    };
    let shared: Vec<&'static str> = still_used_by
        .iter()
        .filter(|other| **other != wm)
        .filter_map(|other| packages(shell, other, family).ok())
        .flatten()
        .collect();
    // Сначала зависимые (`dms-shell-niri` держит `dms-shell`): pacman снимет
    // их одной транзакцией, но порядок в журнале читается естественнее.
    own.into_iter()
        .rev()
        .filter(|p| !shared.contains(p))
        .collect()
}

// ============================================================================
// Где лежит конфиг композитора и что в него вписать
// ============================================================================
//
// ⭐ Управляемый блок — ОБЩИЙ помощник продукта
// (`crate::compositor_autostart`: `upsert_text`/`remove_text`/`launch_line`):
// те же метки, тот же разбор синтаксисов и та же обратимость бит-в-бит, что у
// строк автозапуска самого Rexon. Своей копии здесь нет. Политика заведения
// файла тоже его: sway получает свой `config` с `include` системного, а
// Hyprland и niri без личного конфига не трогаем — при первом входе они
// кладут свой, и файл из одной нашей строки оставил бы человека без клавиш.

/// Личный конфиг композитора: путь, синтаксис, и чем засеять, если его нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: String,
    pub syntax: Syntax,
    /// Файла нет: `Some(преамбула)` — заводить можно, `None` — нельзя.
    pub create_with: Option<Vec<String>>,
    pub exists: bool,
}

/// Файлы, где у композитора может лежать наш блок, в порядке поиска самого
/// композитора (тот же, что у `compositor_autostart::resolve_target`).
pub fn config_files(wm: &str, home: &str, xdg_config_home: Option<&str>) -> Vec<(String, Syntax)> {
    let cfg = crate::utils::config_home(xdg_config_home, home);
    let home = home.trim_end_matches('/');
    match wm {
        // Конфиг i3 sway тоже читает: без этих путей мы завели бы свой
        // `sway/config`, и он заслонил бы конфиг человека.
        "sway" => vec![
            (format!("{home}/.sway/config"), Syntax::Sway),
            (format!("{cfg}/sway/config"), Syntax::Sway),
            (format!("{home}/.i3/config"), Syntax::Sway),
            (format!("{cfg}/i3/config"), Syntax::Sway),
        ],
        "niri" => vec![(format!("{cfg}/niri/config.kdl"), Syntax::NiriKdl)],
        // Hyprland 0.55+: hyprland.lua, если есть, иначе hyprland.conf.
        "hyprland" => vec![
            (format!("{cfg}/hypr/hyprland.lua"), Syntax::HyprLua),
            (format!("{cfg}/hypr/hyprland.conf"), Syntax::HyprConf),
        ],
        _ => Vec::new(),
    }
}

/// Главный конфиг композитора по пробе `exists`.
pub fn config_target(
    wm: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<ConfigFile> {
    let files = config_files(wm, home, xdg_config_home);
    if let Some((path, syntax)) = files.iter().find(|(p, _)| exists(p)) {
        return Some(ConfigFile {
            path: path.clone(),
            syntax: *syntax,
            create_with: None,
            exists: true,
        });
    }
    let (path, syntax) = match wm {
        "sway" => files.get(1)?.clone(),
        _ => files.first()?.clone(),
    };
    let create_with = match wm {
        // Свой файл заслоняет системный — поэтому первым делом подключаем его.
        "sway" if exists("/etc/sway/config") => {
            Some(vec![format!("include {}", crate::themes::sway_path("/etc/sway/config"))])
        }
        _ => None,
    };
    Some(ConfigFile {
        path,
        syntax,
        create_with,
        exists: false,
    })
}

/// Каталог отметок «этот композитор — с этой оболочкой».
pub fn marker_dir(home: &str, xdg_state_home: Option<&str>) -> String {
    let base = match xdg_state_home {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/.local/state", home.trim_end_matches('/')),
    };
    format!("{base}/rexon/shells")
}

/// Отметка режима «композитор с оболочкой».
pub fn marker_path(wm: &str, shell: DesktopShell, home: &str, xdg_state_home: Option<&str>) -> String {
    format!("{}/{wm}-{}", marker_dir(home, xdg_state_home), shell.as_str())
}

/// Можно ли вписать путь отметки в строку запуска без кавычек внутри.
///
/// Команда целиком уходит в `sh -c '…'`, а внутренние кавычки общий помощник
/// экранирует обратной косой, которую sway снимает ещё до оболочки. Поэтому
/// путь обязан состоять из «безопасных» для оболочки знаков (буквы любого
/// алфавита, цифры, `_@%+=:./-`) — домашний каталог с пробелом или кавычкой
/// встречается редко, и в конфиг человека такое мы не понесём.
pub fn path_is_config_safe(path: &str) -> bool {
    path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || "_@%+=:./-".contains(c))
}

/// argv строки автозапуска: оболочка стартует, только если лежит отметка.
pub fn autostart_argv(shell: DesktopShell, marker: &str) -> Vec<String> {
    vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("test -e {marker} && exec {}", launch_command(shell)),
    ]
}

/// Идентификатор управляемого блока оболочки (у каждой свой: стоять могут обе).
pub fn block_id(shell: DesktopShell) -> String {
    format!("shell-{}", shell.as_str())
}

/// Стоит ли в тексте наш блок этой оболочки.
pub fn has_block(text: &str, syntax: Syntax, shell: DesktopShell) -> bool {
    ca::has_block(text, syntax, &block_id(shell))
}

/// Что делать с конфигом композитора при установке оболочки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnablePlan {
    /// Записать файл (`created` — личного конфига не было, он заведён).
    Write {
        path: String,
        content: String,
        created: bool,
    },
    /// Блок уже на месте — ничего не писать.
    AlreadyThere { path: String },
    /// Личного конфига нет, а заводить его нельзя.
    NoConfig,
    /// Строку в этот конфиг не написать (знаки, устройство файла).
    Refused { path: String, reason: Refusal },
}

/// Решить, что сделать с конфигом композитора `wm`, чтобы оболочка
/// запускалась в режиме «с оболочкой». `read` — чтение файла (`None` — нет).
pub fn enable_plan(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    xdg_state_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> EnablePlan {
    let marker = marker_path(wm, shell, home, xdg_state_home);
    if !path_is_config_safe(&marker) {
        return EnablePlan::Refused {
            path: marker,
            reason: Refusal::BadCharacters,
        };
    }
    let exists = |p: &str| read(p).is_some();
    let Some(target) = config_target(wm, home, xdg_config_home, &exists) else {
        return EnablePlan::NoConfig;
    };
    let existing = read(&target.path);
    if existing.is_none() && target.create_with.is_none() {
        return EnablePlan::NoConfig;
    }
    let refused = |reason| EnablePlan::Refused {
        path: target.path.clone(),
        reason,
    };
    let body = match ca::launch_line(target.syntax, &block_id(shell), &autostart_argv(shell, &marker)) {
        Ok(b) => b,
        Err(r) => return refused(r),
    };
    let preamble = target.create_with.clone().unwrap_or_default();
    match ca::upsert_text(existing.as_deref(), target.syntax, &block_id(shell), &body, &preamble) {
        Ok(TextEdit::Write(content)) => EnablePlan::Write {
            path: target.path.clone(),
            content,
            created: existing.is_none(),
        },
        Ok(_) => EnablePlan::AlreadyThere {
            path: target.path.clone(),
        },
        Err(r) => refused(r),
    }
}

/// Что делать с конфигом при «Убрать».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisablePlan {
    /// Конфиг, в котором нашёлся блок (пусто — блока нет нигде).
    pub path: String,
    /// `Unchanged` — трогать нечего, `Write` — новый текст, `Delete` —
    /// файл заводили мы, и без блока он пуст.
    pub edit: TextEdit,
}

/// Найти блок оболочки в конфигах композитора и решить, как его снять.
pub fn disable_plan(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> DisablePlan {
    for (path, syntax) in config_files(wm, home, xdg_config_home) {
        let Some(text) = read(&path) else { continue };
        if has_block(&text, syntax, shell) {
            return DisablePlan {
                edit: ca::remove_text(&text, syntax, &block_id(shell)),
                path,
            };
        }
    }
    DisablePlan {
        path: String::new(),
        edit: TextEdit::Unchanged,
    }
}

/// Стоит ли блок оболочки в конфиге этого композитора.
pub fn block_installed(
    wm: &str,
    shell: DesktopShell,
    home: &str,
    xdg_config_home: Option<&str>,
    read: &dyn Fn(&str) -> Option<String>,
) -> bool {
    !disable_plan(wm, shell, home, xdg_config_home, read).path.is_empty()
}

// ============================================================================
// Предупреждения перед установкой
// ============================================================================

/// О чём предупредить человека ДО установки (разведка 01.10.2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    /// Обе оболочки сами пишут `gsettings … color-scheme` (светлая/тёмная) —
    /// переключатель Rexon они перебьют.
    ColorScheme,
    /// Обе берут на себя уведомления и `StatusNotifierWatcher` (трей), если
    /// имя свободно: своя панель (waybar) может остаться без значков.
    NotificationsAndTray,
    /// Обе рисуют свои обои на фоновом слое — поверх обоев Rexon.
    OwnWallpaper,
    /// DMS под Hyprland при живом `hyprland.lua` при КАЖДОМ запуске переносит
    /// `hyprland.conf` и `hypr/dms/*.conf` в `.dms-backups/`.
    DmsMovesHyprlandConf,
}

/// Предупреждения для установки `shell` в `wm`. `hypr_lua` и `hypr_strays`
/// — пробы `~/.config/hypr` (DMS смотрит именно туда, мимо
/// `XDG_CONFIG_HOME`): есть ли `hyprland.lua` и есть ли рядом
/// `hyprland.conf` или `dms/*.conf`.
pub fn install_warnings(
    shell: DesktopShell,
    wm: &str,
    hypr_lua: bool,
    hypr_strays: bool,
) -> Vec<Warning> {
    let mut out = vec![Warning::OwnWallpaper, Warning::ColorScheme, Warning::NotificationsAndTray];
    if shell == DesktopShell::DankMaterialShell && wm == "hyprland" && hypr_lua && hypr_strays {
        out.push(Warning::DmsMovesHyprlandConf);
    }
    out
}

// ============================================================================
// Состояние оболочки в карточке
// ============================================================================

/// Всё, что карточке нужно знать об оболочке, — собирает вызывающий.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellState {
    /// Стоит ли сам композитор.
    pub wm_installed: bool,
    /// Стоит ли исполняемый файл оболочки.
    pub binary_present: bool,
    /// Стоит ли наш блок в конфиге композитора.
    pub block_present: bool,
    /// Лежит ли отметка «композитор с этой оболочкой».
    pub marker_on: bool,
    /// Работает ли человек сейчас в этом композиторе.
    pub is_current_wm: bool,
}

impl ShellState {
    /// Оболочка поставлена связкой: и пакет, и строка в конфиге.
    pub fn installed(&self) -> bool {
        self.binary_present && self.block_present
    }

    /// Сейчас человек в этом композиторе и с этой оболочкой.
    pub fn running_now(&self) -> bool {
        self.installed() && self.marker_on && self.is_current_wm
    }
}
