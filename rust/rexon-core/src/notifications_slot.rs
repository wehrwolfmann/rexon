//! Запрет подмены приложения уведомлений: кто отвечает на имя
//! `org.freedesktop.Notifications`, когда столов стоит несколько.
//!
//! ЗАЧЕМ. Rexon ставит много рабочих столов сразу, и у каждого свой демон
//! уведомлений со своим файлом D-Bus-активации на ОДНО и то же имя шины
//! (`fr.emersion.mako.service`, `org.freedesktop.mate.Notifications.service`,
//! `org.kde.plasma.Notifications.service`,
//! `org.xfce.xfce4-notifyd.Notifications.service`…). Шина из таких дублей
//! берёт первый по алфавиту и пишет в журнал «Ignoring duplicate name
//! 'org.freedesktop.Notifications' in service file …». Итог, пойманный на
//! машине Wehrwolfmann 12.11.2026: в Plasma, MATE и XFCE уведомление,
//! пришедшее до старта родной оболочки, рисует чужой mako — сверху справа,
//! в чужом виде.
//!
//! ЛЕЧЕНИЕ. Пользовательский каталог `$XDG_DATA_HOME/dbus-1/services`
//! главнее системных, а файл, названный по имени шины, шина берёт прежде
//! прочих. Кладём туда `org.freedesktop.Notifications.service`, который
//! выбирает демон по `$XDG_CURRENT_DESKTOP`: ветки — только для РЕАЛЬНО
//! установленных демонов (команда берётся из их собственного системного
//! файла, так что пути дистрибутива угадывать не нужно), всё остальное —
//! ожидание, пока имя заберёт оболочка (`plasma_waitforname`, если он есть,
//! иначе активация просто не удаётся). GNOME, Cinnamon, Budgie, Deepin и
//! COSMIC держат имя в самой оболочке — им достаточно не мешать.
//!
//! ПРАВИЛА:
//! * файл нужен, только если в системных каталогах БОЛЬШЕ ОДНОГО файла на
//!   это имя; стал один (стол удалили) — наш файл убирается;
//! * файл без нашей метки в первой строке ([`MARKER`]) — чужой (его
//!   положил человек) и не трогается никогда; байт-в-байт равный нашему
//!   тексту — наш по определению (метка в нём та же);
//! * ничего не убиваем: после смены файла шина только перечитывает
//!   конфигурацию (`ReloadConfig`), живой демон доживает до конца сеанса.
//!
//! Где зовётся: после любой операции над столами во вкладке «Рабочие
//! столы» и при старте окна Rexon ([`sync_live`]); удаление Rexon убирает
//! наш файл (`installer_exec::release_session_guard`).

use std::path::{Path, PathBuf};

use crate::{log_info, log_warn};

/// Имя шины, которое делят демоны уведомлений.
pub const BUS_NAME: &str = "org.freedesktop.Notifications";

/// Первая строка нашего файла — по ней файл узнаётся как наш.
pub const MARKER: &str = "# Rexon: notifications-slot";

/// Системные каталоги файлов D-Bus-активации сеансовой шины.
pub const SYSTEM_SERVICE_DIRS: &[&str] = &["/usr/share/dbus-1/services", "/usr/local/share/dbus-1/services"];

/// Демон уведомлений: имя его программы (последний сегмент пути в `Exec=`)
/// и значения `$XDG_CURRENT_DESKTOP`, на которых он родной.
///
/// Одна таблица на продукт (правило #12): новый демон — строкой здесь.
/// Порядок важен: если на один стол претендуют двое (mako и dunst под
/// Hyprland), ветку получает тот, кто выше.
pub struct DaemonInfo {
    pub binary: &'static str,
    pub desktops: &'static [&'static str],
}

/// Ожидатель имени оболочкой Plasma — он же запасная ветка для прочих столов.
pub const SHELL_WAITER: &str = "plasma_waitforname";

/// Столы на wlroots и родне, где демон уведомлений — отдельная программа.
const WLROOTS_DESKTOPS: &[&str] = &["Hyprland", "sway", "labwc", "niri", "river", "wlroots", "Wayfire"];

pub const DAEMONS: &[DaemonInfo] = &[
    DaemonInfo { binary: SHELL_WAITER, desktops: &["KDE"] },
    DaemonInfo { binary: "mate-notification-daemon", desktops: &["MATE"] },
    DaemonInfo { binary: "xfce4-notifyd", desktops: &["XFCE"] },
    DaemonInfo { binary: "lxqt-notificationd", desktops: &["LXQt"] },
    DaemonInfo { binary: "mako", desktops: WLROOTS_DESKTOPS },
    DaemonInfo { binary: "swaync", desktops: WLROOTS_DESKTOPS },
    DaemonInfo { binary: "dunst", desktops: WLROOTS_DESKTOPS },
];

/// Один системный файл активации на [`BUS_NAME`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Имя файла (для журнала и порядка).
    pub file: String,
    /// Строка `Exec=` как есть.
    pub exec: String,
    /// Программа из `Exec=` существует на диске.
    pub installed: bool,
}

impl Provider {
    /// Последний сегмент пути программы из `Exec=`.
    pub fn binary(&self) -> &str {
        let first = self.exec.split_whitespace().next().unwrap_or("");
        first.rsplit('/').next().unwrap_or(first)
    }
}

/// Разбор файла активации: `(Name, Exec)` из раздела `[D-BUS Service]`.
pub fn parse_service(text: &str) -> (Option<String>, Option<String>) {
    let mut in_section = false;
    let (mut name, mut exec) = (None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[D-BUS Service]";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec = Some(v.trim().to_string());
        }
    }
    (name, exec)
}

/// Наш ли это файл: первая строка — метка.
pub fn is_ours(text: &str) -> bool {
    text.lines().next().map(str::trim_end) == Some(MARKER)
}

/// Команду можно вставить в `sh -c '…'` как есть: без одинарных кавычек
/// (иначе она разорвала бы обёртку) и без переводов строки.
fn exec_is_safe(exec: &str) -> bool {
    !exec.is_empty() && !exec.contains('\'') && !exec.contains('\n')
}

/// Текст нашего файла по найденным провайдерам. `None` — делать нечего
/// (провайдер один или ни одного): тогда шина и без нас не ошибётся.
pub fn render(providers: &[Provider]) -> Option<String> {
    if providers.len() < 2 {
        return None;
    }
    let mut sorted: Vec<&Provider> = providers.iter().filter(|p| p.installed && exec_is_safe(&p.exec)).collect();
    sorted.sort_by(|a, b| a.file.cmp(&b.file));
    let by_binary = |bin: &str| sorted.iter().find(|p| p.binary() == bin).copied();

    let mut taken: Vec<&str> = Vec::new();
    let mut branches: Vec<String> = Vec::new();
    for daemon in DAEMONS {
        let Some(provider) = by_binary(daemon.binary) else {
            continue;
        };
        let desktops: Vec<&str> = daemon.desktops.iter().copied().filter(|d| !taken.contains(d)).collect();
        if desktops.is_empty() {
            continue;
        }
        taken.extend(&desktops);
        let pattern: Vec<String> = desktops.iter().map(|d| format!("*:{d}:*")).collect();
        branches.push(format!("{}) exec {};;", pattern.join("|"), provider.exec));
    }
    // Ни одного знакомого демона: наш файл только отнял бы имя у всех —
    // пусть шина выбирает сама.
    if branches.is_empty() {
        return None;
    }
    // Прочие столы: ждать, пока имя заберёт оболочка. Без ожидателя Plasma —
    // активация просто не удаётся (имя держит сама оболочка стола).
    let fallback = match by_binary(SHELL_WAITER) {
        Some(p) => format!("exec {}", p.exec),
        None => "exit 1".to_string(),
    };
    branches.push(format!("*) {fallback};;"));

    let names: Vec<&str> = sorted.iter().map(|p| p.file.as_str()).collect();
    Some(format!(
        "{MARKER}\n\
         # Several notification daemons claim {BUS_NAME}: {}.\n\
         # This file picks the one that belongs to the current desktop.\n\
         # Rexon keeps it up to date; delete it to return to the system default.\n\
         [D-BUS Service]\n\
         Name={BUS_NAME}\n\
         Exec=/bin/sh -c 'case \":$XDG_CURRENT_DESKTOP:\" in {} esac'\n",
        names.join(", "),
        branches.join(" "),
    ))
}

/// Что сделать с пользовательским файлом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Всё уже как надо либо файл чужой.
    Nothing,
    /// Записать этот текст.
    Write(String),
    /// Убрать наш файл: дублей больше нет.
    Remove,
}

/// Решение по провайдерам и нынешнему содержимому файла (`None` — нет файла).
pub fn decide(providers: &[Provider], existing: Option<&str>) -> Action {
    let desired = render(providers);
    match (existing, desired) {
        (None, None) => Action::Nothing,
        (None, Some(text)) => Action::Write(text),
        (Some(cur), desired) if !is_ours(cur) && desired.as_deref() != Some(cur) => Action::Nothing,
        (Some(cur), Some(text)) if cur == text => Action::Nothing,
        (Some(_), Some(text)) => Action::Write(text),
        (Some(_), None) => Action::Remove,
    }
}

/// Где искать и куда писать.
#[derive(Debug, Clone)]
pub struct SlotPaths {
    pub system_dirs: Vec<PathBuf>,
    pub user_file: PathBuf,
}

impl SlotPaths {
    /// Пути живого сеанса: `$XDG_DATA_HOME` (или `~/.local/share`).
    pub fn from_env(home: &str, xdg_data_home: &str) -> Self {
        let data = crate::utils::data_home(Some(xdg_data_home).filter(|v| v.starts_with('/')), home);
        SlotPaths {
            system_dirs: SYSTEM_SERVICE_DIRS.iter().map(PathBuf::from).collect(),
            user_file: Path::new(&data).join("dbus-1/services").join(format!("{BUS_NAME}.service")),
        }
    }
}

/// Системные файлы активации на [`BUS_NAME`]. Файл, совпадающий по имени в
/// нескольких каталогах, считается один раз (первый каталог главнее).
pub fn scan_providers(dirs: &[PathBuf]) -> Vec<Provider> {
    let mut out: Vec<Provider> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| n.ends_with(".service"))
            .collect();
        names.sort();
        for file in names {
            if out.iter().any(|p| p.file == file) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(dir.join(&file)) else {
                continue;
            };
            let (name, exec) = parse_service(&text);
            if name.as_deref() != Some(BUS_NAME) {
                continue;
            }
            let exec = exec.unwrap_or_default();
            let program = exec.split_whitespace().next().unwrap_or("");
            let installed = program.starts_with('/') && Path::new(program).is_file();
            out.push(Provider { file, exec, installed });
        }
    }
    out
}

/// Привести пользовательский файл к решению. `Ok(action)` — что сделано
/// (`Nothing` — ничего не менялось).
pub fn sync(paths: &SlotPaths) -> Result<Action, String> {
    let providers = scan_providers(&paths.system_dirs);
    let existing = std::fs::read(&paths.user_file).ok().map(|raw| String::from_utf8_lossy(&raw).into_owned());
    let action = decide(&providers, existing.as_deref());
    let target = paths.user_file.to_string_lossy();
    match &action {
        Action::Nothing => {}
        Action::Write(text) => {
            if let Some(dir) = paths.user_file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            crate::utils::atomic_write_str_err(&target, text).map_err(|e| format!("{target}: {e}"))?;
        }
        Action::Remove => {
            std::fs::remove_file(&paths.user_file).map_err(|e| format!("{target}: {e}"))?;
        }
    }
    Ok(action)
}

/// Убрать наш файл (удаление Rexon). `true` — файл был нашим и убран.
pub fn release(user_file: &Path) -> bool {
    match std::fs::read(user_file) {
        Ok(raw) if is_ours(&String::from_utf8_lossy(&raw)) => std::fs::remove_file(user_file).is_ok(),
        _ => false,
    }
}

/// Живой сеанс: привести файл в порядок и, если он поменялся, попросить
/// шину перечитать конфигурацию. Ничего не убивает. Сбой — только в журнал.
pub fn sync_live() {
    let paths = SlotPaths::from_env(
        &std::env::var("HOME").unwrap_or_default(),
        &std::env::var("XDG_DATA_HOME").unwrap_or_default(),
    );
    if !paths.user_file.is_absolute() {
        return;
    }
    match sync(&paths) {
        Ok(Action::Nothing) => {}
        Ok(action) => {
            let what = if action == Action::Remove { "убран" } else { "записан" };
            log_info!("notifications", "файл выбора демона уведомлений {what}: {}", paths.user_file.display());
            reload_bus_live();
        }
        Err(e) => log_warn!("notifications", "файл выбора демона уведомлений: {e}"),
    }
}

/// `ReloadConfig` сеансовой шины — та же команда, что у FileManager1.
pub fn reload_bus_live() {
    let spec = crate::utils::RunSpec {
        argv: crate::appdefaults::busctl_reload_cmd(),
        output: crate::utils::RunOutput::Capture,
        timeout_secs: Some(10),
        ..crate::utils::RunSpec::default()
    };
    crate::utils::run(&spec, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-notify-slot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Стенд: системный каталог с файлами и программами внутри стенда.
    struct Bench {
        root: PathBuf,
        paths: SlotPaths,
    }

    impl Bench {
        fn new(tag: &str) -> Self {
            let root = scratch(tag);
            let sys = root.join("sys");
            std::fs::create_dir_all(&sys).unwrap();
            std::fs::create_dir_all(root.join("bin")).unwrap();
            let paths = SlotPaths {
                system_dirs: vec![sys, root.join("missing")],
                user_file: root.join("home/dbus-1/services").join(format!("{BUS_NAME}.service")),
            };
            Bench { root, paths }
        }

        /// Файл активации `file` на программу `bin` (с аргументами `args`).
        fn provider(&self, file: &str, bin: &str, args: &str, with_binary: bool) -> String {
            let program = self.root.join("bin").join(bin);
            if with_binary {
                std::fs::write(&program, "").unwrap();
            }
            let exec = format!("{}{args}", program.display());
            std::fs::write(
                self.paths.system_dirs[0].join(file),
                format!("[D-BUS Service]\nName={BUS_NAME}\nExec={exec}\n"),
            )
            .unwrap();
            exec
        }

        fn user_text(&self) -> Option<String> {
            std::fs::read_to_string(&self.paths.user_file).ok()
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn p(file: &str, exec: &str) -> Provider {
        Provider { file: file.into(), exec: exec.into(), installed: true }
    }

    #[test]
    fn render_branches_only_for_installed_daemons() {
        let mut gone = p("org.xfce.xfce4-notifyd.Notifications.service", "/usr/lib/xfce4/notifyd/xfce4-notifyd");
        gone.installed = false;
        let text = render(&[
            p("fr.emersion.mako.service", "/usr/bin/mako"),
            p(
                "org.freedesktop.mate.Notifications.service",
                "/usr/lib/mate-notification-daemon/mate-notification-daemon --idle-exit",
            ),
            p("org.kde.plasma.Notifications.service", "/usr/bin/plasma_waitforname org.freedesktop.Notifications"),
            gone,
        ])
        .unwrap();
        assert!(is_ours(&text));
        assert!(text.contains("\nName=org.freedesktop.Notifications\n"));
        let exec = text.lines().find(|l| l.starts_with("Exec=")).unwrap();
        assert_eq!(
            exec,
            "Exec=/bin/sh -c 'case \":$XDG_CURRENT_DESKTOP:\" in \
             *:KDE:*) exec /usr/bin/plasma_waitforname org.freedesktop.Notifications;; \
             *:MATE:*) exec /usr/lib/mate-notification-daemon/mate-notification-daemon --idle-exit;; \
             *:Hyprland:*|*:sway:*|*:labwc:*|*:niri:*|*:river:*|*:wlroots:*|*:Wayfire:*) exec /usr/bin/mako;; \
             *) exec /usr/bin/plasma_waitforname org.freedesktop.Notifications;; esac'"
        );
        assert!(!text.contains("xfce4-notifyd;;"), "нет программы — нет ветки");
    }

    #[test]
    fn render_without_plasma_falls_back_to_failure() {
        let text = render(&[p("a.service", "/usr/bin/mako"), p("b.service", "/usr/bin/dunst")]).unwrap();
        assert!(text.contains("*) exit 1;;"));
        // На один стол двое: ветку берёт тот, кто выше в таблице.
        assert!(text.contains("exec /usr/bin/mako;;"));
        assert!(!text.contains("exec /usr/bin/dunst"));
    }

    #[test]
    fn render_skips_exec_that_would_break_quoting() {
        let text = render(&[p("a.service", "/usr/bin/mako --x 'y'"), p("b.service", "/usr/bin/xfce4-notifyd")]).unwrap();
        assert!(!text.contains("mako"));
        assert!(text.contains("*:XFCE:*) exec /usr/bin/xfce4-notifyd;;"));
    }

    #[test]
    fn single_provider_needs_no_file() {
        assert_eq!(render(&[p("a.service", "/usr/bin/mako")]), None);
        // Двое, но оба незнакомы — файл только отнял бы имя у всех.
        assert_eq!(render(&[p("a.service", "/usr/bin/foo"), p("b.service", "/usr/bin/bar")]), None);
        assert_eq!(decide(&[p("a.service", "/usr/bin/mako")], None), Action::Nothing);
    }

    #[test]
    fn decide_respects_foreign_and_cleans_own() {
        let two = [p("a.service", "/usr/bin/mako"), p("b.service", "/usr/bin/xfce4-notifyd")];
        let ours = render(&two).unwrap();
        assert_eq!(decide(&two, None), Action::Write(ours.clone()));
        assert_eq!(decide(&two, Some(&ours)), Action::Nothing);
        // Чужой файл — не трогаем ни при дублях, ни без них.
        let foreign = "[D-BUS Service]\nName=org.freedesktop.Notifications\nExec=/usr/bin/mako\n";
        assert_eq!(decide(&two, Some(foreign)), Action::Nothing);
        assert_eq!(decide(&two[..1], Some(foreign)), Action::Nothing);
        // Наш устаревший — переписываем; дублей не стало — убираем.
        let stale = format!("{MARKER}\n[D-BUS Service]\nExec=/bin/false\n");
        assert_eq!(decide(&two, Some(&stale)), Action::Write(ours));
        assert_eq!(decide(&two[..1], Some(&stale)), Action::Remove);
    }

    #[test]
    fn parse_service_reads_only_its_section() {
        let (name, exec) = parse_service("[Other]\nName=x\n[D-BUS Service]\nName=org.freedesktop.Notifications\nExec=/usr/bin/mako\n");
        assert_eq!(name.as_deref(), Some(BUS_NAME));
        assert_eq!(exec.as_deref(), Some("/usr/bin/mako"));
    }

    #[test]
    fn sync_writes_when_duplicates_and_removes_when_gone() {
        let b = Bench::new("sync");
        let mako = b.provider("fr.emersion.mako.service", "mako", "", true);
        let notifyd = b.provider("org.xfce.xfce4-notifyd.Notifications.service", "xfce4-notifyd", "", true);
        // Посторонний файл на другое имя — не провайдер.
        std::fs::write(b.paths.system_dirs[0].join("org.other.service"), "[D-BUS Service]\nName=org.other\nExec=/x\n").unwrap();
        assert_eq!(scan_providers(&b.paths.system_dirs).len(), 2);

        assert!(matches!(sync(&b.paths).unwrap(), Action::Write(_)));
        let text = b.user_text().unwrap();
        assert!(text.contains(&format!("exec {mako};;")));
        assert!(text.contains(&format!("*:XFCE:*) exec {notifyd};;")));
        // Повторный прогон ничего не меняет.
        assert_eq!(sync(&b.paths).unwrap(), Action::Nothing);

        // Стол удалили: дублей нет — наш файл уходит.
        std::fs::remove_file(b.paths.system_dirs[0].join("fr.emersion.mako.service")).unwrap();
        assert_eq!(sync(&b.paths).unwrap(), Action::Remove);
        assert!(b.user_text().is_none());
    }

    #[test]
    fn sync_never_touches_foreign_file() {
        let b = Bench::new("foreign");
        b.provider("a.service", "mako", "", true);
        b.provider("b.service", "xfce4-notifyd", "", true);
        std::fs::create_dir_all(b.paths.user_file.parent().unwrap()).unwrap();
        let foreign = "[D-BUS Service]\nName=org.freedesktop.Notifications\nExec=/usr/bin/mako\n";
        std::fs::write(&b.paths.user_file, foreign).unwrap();
        assert_eq!(sync(&b.paths).unwrap(), Action::Nothing);
        assert_eq!(b.user_text().as_deref(), Some(foreign));
        assert!(!release(&b.paths.user_file));
        assert_eq!(b.user_text().as_deref(), Some(foreign));
    }

    #[test]
    fn release_removes_only_ours() {
        let b = Bench::new("release");
        b.provider("a.service", "mako", "", true);
        b.provider("b.service", "plasma_waitforname", " org.freedesktop.Notifications", true);
        sync(&b.paths).unwrap();
        assert!(b.user_text().is_some_and(|t| is_ours(&t)));
        assert!(release(&b.paths.user_file));
        assert!(b.user_text().is_none());
        assert!(!release(&b.paths.user_file), "второй раз убирать нечего");
    }

    #[test]
    fn missing_binary_gets_no_branch_but_counts_as_duplicate() {
        let b = Bench::new("stale");
        b.provider("a.service", "mako", "", false);
        let kde = b.provider("b.service", "plasma_waitforname", " org.freedesktop.Notifications", true);
        assert!(matches!(sync(&b.paths).unwrap(), Action::Write(_)));
        let text = b.user_text().unwrap();
        assert!(!text.contains("bin/mako"));
        assert!(text.contains(&format!("*) exec {kde};;")));
    }

    #[test]
    fn user_file_follows_xdg_data_home() {
        let p = SlotPaths::from_env("/home/u", "/data");
        assert_eq!(p.user_file, Path::new("/data/dbus-1/services/org.freedesktop.Notifications.service"));
        let p = SlotPaths::from_env("/home/u", "");
        assert_eq!(p.user_file, Path::new("/home/u/.local/share/dbus-1/services/org.freedesktop.Notifications.service"));
    }
}
