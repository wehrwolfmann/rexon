//! Preventing the notification application from being hijacked: who answers
//! to the name `org.freedesktop.Notifications` when several desktops are
//! installed.
//!
//! WHY. Rexon installs many desktops at once, and each has its own notification
//! daemon with its own D-Bus activation file for ONE AND THE SAME bus name
//! (`fr.emersion.mako.service`, `org.freedesktop.mate.Notifications.service`,
//! `org.kde.plasma.Notifications.service`,
//! `org.xfce.xfce4-notifyd.Notifications.service`…). Faced with such duplicates
//! the bus takes the first one alphabetically and logs "Ignoring duplicate name
//! 'org.freedesktop.Notifications' in service file …". The result, caught on
//! Wehrwolfmann's machine on 12.11.2026: in Plasma, MATE and XFCE a
//! notification that arrived before the native shell started was drawn by a
//! foreign mako — top right, in a foreign look.
//!
//! THE CURE. The user directory `$XDG_DATA_HOME/dbus-1/services` takes
//! precedence over the system ones, and a file named after the bus name is
//! taken before all others. We put `org.freedesktop.Notifications.service`
//! there, which picks the daemon by `$XDG_CURRENT_DESKTOP`: branches exist only
//! for ACTUALLY installed daemons (the command is taken from their own system
//! file, so there is no need to guess the distribution's paths); everything
//! else waits until the shell takes the name (`plasma_waitforname` if it is
//! present, otherwise activation simply fails). GNOME, Cinnamon, Budgie, Deepin
//! and COSMIC hold the name in the shell itself — it is enough not to get in
//! their way.
//!
//! RULES:
//! * the file is needed only if the system directories have MORE THAN ONE file
//!   for this name; once there is one (a desktop was removed), our file is
//!   removed;
//! * a file without our marker in the first line ([`MARKER`]) is foreign (the
//!   person put it there) and is never touched; a file byte-for-byte equal to
//!   our text is ours by definition (it carries the same marker);
//! * we kill nothing: after the file changes, the bus only reloads its
//!   configuration (`ReloadConfig`), and the running daemon lives until the end
//!   of the session.
//!
//! Where it is called: after any operation on desktops in the "Desktops" tab
//! and when the Rexon window starts ([`sync_live`]); uninstalling Rexon removes
//! our file (`installer_exec::release_session_guard`).

use std::path::{Path, PathBuf};

use crate::{log_info, log_warn};

/// The bus name shared by notification daemons.
pub const BUS_NAME: &str = "org.freedesktop.Notifications";

/// First line of our file — the file is recognized as ours by it.
pub const MARKER: &str = "# Rexon: notifications-slot";

/// System directories of session-bus D-Bus activation files.
pub const SYSTEM_SERVICE_DIRS: &[&str] = &["/usr/share/dbus-1/services", "/usr/local/share/dbus-1/services"];

/// A notification daemon: the name of its program (the last path segment in
/// `Exec=`) and the `$XDG_CURRENT_DESKTOP` values on which it is native.
///
/// One table for the product (rule #12): a new daemon is a line here.
/// Order matters: if two daemons claim one desktop (mako and dunst under
/// Hyprland), the branch goes to the one listed higher.
pub struct DaemonInfo {
    pub binary: &'static str,
    pub desktops: &'static [&'static str],
}

/// Plasma's shell name waiter — also the fallback branch for other desktops.
pub const SHELL_WAITER: &str = "plasma_waitforname";

/// Desktops on wlroots and relatives, where the notification daemon is a
/// separate program.
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

/// One system activation file for [`BUS_NAME`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// File name (for the log and for ordering).
    pub file: String,
    /// The `Exec=` line as is.
    pub exec: String,
    /// The program from `Exec=` exists on disk.
    pub installed: bool,
}

impl Provider {
    /// The last path segment of the program from `Exec=`.
    pub fn binary(&self) -> &str {
        let first = self.exec.split_whitespace().next().unwrap_or("");
        first.rsplit('/').next().unwrap_or(first)
    }
}

/// Parsing an activation file: `(Name, Exec)` from the `[D-BUS Service]` section.
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

/// Whether this is our file: the first line is the marker.
pub fn is_ours(text: &str) -> bool {
    text.lines().next().map(str::trim_end) == Some(MARKER)
}

/// The command can be inserted into `sh -c '…'` as is: without single quotes
/// (otherwise it would break the wrapper) and without line breaks.
fn exec_is_safe(exec: &str) -> bool {
    !exec.is_empty() && !exec.contains('\'') && !exec.contains('\n')
}

/// Text of our file for the providers found. `None` — nothing to do (one
/// provider or none): then the bus will not go wrong without us.
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
    // Not a single known daemon: our file would only take the name away from
    // everyone — let the bus choose by itself.
    if branches.is_empty() {
        return None;
    }
    // Other desktops: wait until the shell takes the name. Without Plasma's
    // waiter, activation simply fails (the desktop's shell holds the name itself).
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

/// What to do with the user file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Everything is already as it should be, or the file is foreign.
    Nothing,
    /// Write this text.
    Write(String),
    /// Remove our file: there are no more duplicates.
    Remove,
}

/// Decision based on the providers and the file's current contents (`None` —
/// no file).
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

/// Where to look and where to write.
#[derive(Debug, Clone)]
pub struct SlotPaths {
    pub system_dirs: Vec<PathBuf>,
    pub user_file: PathBuf,
}

impl SlotPaths {
    /// Paths of the live session: `$XDG_DATA_HOME` (or `~/.local/share`).
    pub fn from_env(home: &str, xdg_data_home: &str) -> Self {
        let data = crate::utils::data_home(Some(xdg_data_home).filter(|v| v.starts_with('/')), home);
        SlotPaths {
            system_dirs: SYSTEM_SERVICE_DIRS.iter().map(PathBuf::from).collect(),
            user_file: Path::new(&data).join("dbus-1/services").join(format!("{BUS_NAME}.service")),
        }
    }
}

/// System activation files for [`BUS_NAME`]. A file with the same name in
/// several directories counts once (the first directory takes precedence).
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

/// Bring the user file in line with the decision. `Ok(action)` — what was done
/// (`Nothing` — nothing changed).
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

/// Remove our file (Rexon uninstall). `true` — the file was ours and was removed.
pub fn release(user_file: &Path) -> bool {
    match std::fs::read(user_file) {
        Ok(raw) if is_ours(&String::from_utf8_lossy(&raw)) => std::fs::remove_file(user_file).is_ok(),
        _ => false,
    }
}

/// Live session: put the file in order and, if it changed, ask the bus to
/// reload its configuration. Kills nothing. A failure goes only to the log.
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

/// `ReloadConfig` of the session bus — the same command as for FileManager1.
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

    /// Test stand: a system directory with files and programs inside the stand.
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

        /// Activation file `file` for program `bin` (with arguments `args`).
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
        // Two for one desktop: the branch goes to the one higher in the table.
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
        // Two, but both unknown — the file would only take the name away from everyone.
        assert_eq!(render(&[p("a.service", "/usr/bin/foo"), p("b.service", "/usr/bin/bar")]), None);
        assert_eq!(decide(&[p("a.service", "/usr/bin/mako")], None), Action::Nothing);
    }

    #[test]
    fn decide_respects_foreign_and_cleans_own() {
        let two = [p("a.service", "/usr/bin/mako"), p("b.service", "/usr/bin/xfce4-notifyd")];
        let ours = render(&two).unwrap();
        assert_eq!(decide(&two, None), Action::Write(ours.clone()));
        assert_eq!(decide(&two, Some(&ours)), Action::Nothing);
        // A foreign file is not touched, with or without duplicates.
        let foreign = "[D-BUS Service]\nName=org.freedesktop.Notifications\nExec=/usr/bin/mako\n";
        assert_eq!(decide(&two, Some(foreign)), Action::Nothing);
        assert_eq!(decide(&two[..1], Some(foreign)), Action::Nothing);
        // Our outdated one is rewritten; once the duplicates are gone, it is removed.
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
        // An unrelated file for another name is not a provider.
        std::fs::write(b.paths.system_dirs[0].join("org.other.service"), "[D-BUS Service]\nName=org.other\nExec=/x\n").unwrap();
        assert_eq!(scan_providers(&b.paths.system_dirs).len(), 2);

        assert!(matches!(sync(&b.paths).unwrap(), Action::Write(_)));
        let text = b.user_text().unwrap();
        assert!(text.contains(&format!("exec {mako};;")));
        assert!(text.contains(&format!("*:XFCE:*) exec {notifyd};;")));
        // A repeated run changes nothing.
        assert_eq!(sync(&b.paths).unwrap(), Action::Nothing);

        // A desktop was removed: no duplicates — our file goes away.
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
