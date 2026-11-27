//! Port of `rexon/utils.py` — shared utilities: logging, normalizing user
//! paths, the SINGLE lookup of external tools and calls to the D-Bus session
//! bus.
//!
//! ⚠️ The project NO LONGER has the `REXON_RUST_BACKEND=off` switch. It sent
//! the shims to Python copies of the logic, but no copies are left in any
//! module: there is nothing to turn off, and the variable's only effect would
//! be «break the program». It was removed together with the copies — and so
//! was its golden reference here.
//!
//! The rest is real logic (path parsing, iterating over binary synonyms,
//! parsing the `dbus-send` answer, building argv), and it is ported entirely.
//! Launching processes and configuring `logging` stay in Python: the core
//! adds nothing there except an extra hop through the bridge.
//!
//! ⚠️ Differs from Python in ONE place: the execute permission is checked by
//! the `x` bit in the file mode, not by calling `access(2)` (the core has no
//! libc dependency, and `std` gives no access to `access`). For distribution
//! binaries (mode 0755) this is the same; only a file with a mode like 0700
//! owned by someone else could diverge. So Python, having got «not found»
//! from the core, searches further by itself — a false «the tool is missing»
//! because of the core is impossible (see `rexon/utils.py::resolve_tool`).

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ============================================================================
// Logging
// ============================================================================

/// Name of the application's root logger.
pub const LOGGER_NAME: &str = "rexon";

/// Log record format.
pub const LOG_FORMAT: &str = "%(asctime)s [%(name)s] %(levelname)s: %(message)s";

/// Timestamp format.
pub const LOG_DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Log level: the numeric values match Python's `logging`.
pub const LEVEL_DEBUG: i32 = 10;
/// Warning.
pub const LEVEL_WARNING: i32 = 30;

// ============================================================================
// User paths (the SINGLE expander for the whole core)
// ============================================================================

/// A directory without trailing `/`, but the root stays the root.
///
/// Matches what `os.path.expanduser` does with the `$HOME` value
/// (`userhome.rstrip('/')`), and also brings `$XDG_*_HOME` values to one
/// form: with a trailing slash in the variable, paths were later glued with
/// `//`.
fn norm_dir(dir: &str) -> &str {
    if !dir.is_empty() && dir.chars().all(|c| c == '/') {
        return "/";
    }
    dir.trim_end_matches('/')
}

/// Joining a path exactly like `os.path.join(a, b)` for a relative `b`.
///
/// A separate function because a naive `format!("{a}/{b}")` gives `//x` on
/// the root, and on an empty `b` — `a/` instead of `a` (or vice versa): the
/// divergence from Python would already show up in config paths.
fn join_path(dir: &str, tail: &str) -> String {
    if tail.starts_with('/') {
        return tail.to_string();
    }
    if dir.is_empty() || dir.ends_with('/') {
        return format!("{dir}{tail}");
    }
    format!("{dir}/{tail}")
}

/// Root of user configs: `$XDG_CONFIG_HOME` or `~/.config`.
///
/// ⚠️ The SINGLE point. Previously `~/.config` was hard-coded into each module
/// in place, and with a non-standard `XDG_CONFIG_HOME` paths drifted away
/// from the real files: a systemd unit landed where `systemctl --user enable`
/// does not look for it (autostart of the wallpaper daemon and the splash
/// silently stayed off), the autostart .desktop and the KWin rule were
/// written past the live files, `mimeapps.list` profiles — past the
/// directory the DEs read them from. Build new paths to user configs ONLY
/// from here (rule #12).
pub fn config_home(xdg_config_home: Option<&str>, home: &str) -> String {
    match xdg_config_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".config"),
    }
}

/// Root of user data: `$XDG_DATA_HOME` or `~/.local/share`.
///
/// The pair of [`config_home`] and the same SINGLE source. Introduced for the
/// same reasons: the path was hard-coded as a literal in more than a hundred
/// places, and with a non-standard `XDG_DATA_HOME` (Flatpak, Steam runtime,
/// custom profiles) themes, cursors, fonts, plasmoids and `.rxn` extensions
/// were installed past the directory the system reads — the install
/// «succeeded», but there was no result.
pub fn data_home(xdg_data_home: Option<&str>, home: &str) -> String {
    match xdg_data_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".local/share"),
    }
}

/// Root of the user cache: `$XDG_CACHE_HOME` or `~/.cache`. The third next to
/// [`config_home`] and [`data_home`], with the same parsing.
pub fn cache_home(xdg_cache_home: Option<&str>, home: &str) -> String {
    match xdg_cache_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".cache"),
    }
}

/// Name of the Rexon folder inside the config root. One for the whole
/// product: both the settings path and the removal list take it from here.
pub const REXON_DIR_NAME: &str = "rexon";

/// Rexon settings directory: `<`[`config_home`]`>/rexon`.
///
/// ⚠️ The SINGLE point for ALL Rexon stores in configs (`settings.json`,
/// `favorites.json`, `bridge.json`, `bridge_commands/`, `screenshots/`,
/// `extensions/`, `themes.json`, `ruffle/`, `crashes/`…). Previously half of
/// the modules glued the path themselves — some through
/// `expand_user`/`config_home` (i.e. by `$XDG_CONFIG_HOME`), some with the
/// literal `~/.config/rexon` from a bare `$HOME`. On a machine with a
/// non-standard `XDG_CONFIG_HOME` there were TWO different settings folders:
/// one half of the product wrote into one, the other read the other, and
/// removing Rexon deleted only the XDG one. Build new paths to Rexon data in
/// configs ONLY from here (rule #12).
pub fn rexon_config_dir_at(xdg_config_home: Option<&str>, home: &str) -> String {
    join_path(&config_home(xdg_config_home, home), REXON_DIR_NAME)
}

/// Config root from the process ENVIRONMENT (`$XDG_CONFIG_HOME`, otherwise
/// `$HOME/.config`; an empty/missing `$HOME` → the current directory, like
/// `os.path.expanduser`).
///
/// The only place in the core where the config root is taken from the
/// environment: previously these four lines were copied into `settings.rs`,
/// `qt_extensions.rs`, `favorites.rs` and `constants.rs` — with different
/// handling of an empty string.
pub fn config_home_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let home = if home.is_empty() { ".".to_string() } else { home };
    config_home(env_nonempty("XDG_CONFIG_HOME").as_deref(), &home)
}

/// [`rexon_config_dir_at`] from the process environment.
pub fn rexon_config_dir() -> PathBuf {
    PathBuf::from(join_path(&config_home_env(), REXON_DIR_NAME))
}

// ============================================================================
// Writing to the customer's profile — ONE door
// ============================================================================

/// Name suffix of the atomic write's temporary file. The name starts with a
/// dot (a hidden file) and does NOT repeat the target's whole name, so
/// directory walks that pick files by prefix or extension (`crash_*.log`,
/// `*.json`) do not see a half-written file.
fn atomic_tmp_name(name: &str) -> String {
    format!(".{name}.part-{}", std::process::id())
}

/// Atomically write text: a temporary file in the same directory → `fsync` →
/// `rename`. On error the temporary file is removed, the final one is untouched.
///
/// ⚠️ **The ONLY door for writing to the customer's profile**
/// (`~/.config/rexon`, `~/.local/share/rexon`, and also desktop settings and
/// default applications that Rexon rewrites on the person's behalf). A plain
/// `fs::write` first TRUNCATES the file and only then fills it: a window
/// closed in that millisecond or a power loss leaves half on disk. Half a
/// `settings.json` is the customer's lost settings, half a `bridge.json` — a
/// bridge that stopped answering, half an autostart `.desktop` — a dead line
/// that comes alive only at session login, when there is nobody to explain
/// the breakage.
///
/// The temporary file is put NEXT TO the target, not in `/tmp`: `rename` is
/// atomic only within one file system, and `/tmp` here is RAM (tmpfs), i.e. a
/// different one. Through `/tmp` a copy would happen instead of a rename, and
/// the truncated file would come back exactly where it was being removed from.
///
/// `fsync` before the rename is not overcaution: without it `rename` may reach
/// the disk before the contents, and after a power failure in place of the
/// whole old file there will be a whole new one — of zeros.
///
/// The directory is NOT created: a missing directory is an error, not a
/// reason to silently set it up in an unexpected place. Whoever needs
/// `mkdir -p` does it themselves.
pub fn atomic_write(path: &str, text: &str) -> std::io::Result<()> {
    atomic_write_bytes(path, text.as_bytes())
}

/// Where to actually write: a symlink is resolved to its target.
///
/// ⚠️ This is NOT nitpicking. Replacing the name hits the link itself: a
/// plain write went through it and filled the file at the other end, while
/// `rename` would put a plain file in its place — and for a person who keeps
/// configs in their own store and places them with links, the connection to
/// the store would silently break. Only an existing link is resolved; a
/// broken link (no target) stays as is, and that is what gets replaced.
fn atomic_target(path: &Path) -> std::borrow::Cow<'_, Path> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => match fs::canonicalize(path) {
            Ok(real) => std::borrow::Cow::Owned(real),
            Err(_) => std::borrow::Cow::Borrowed(path),
        },
        _ => std::borrow::Cow::Borrowed(path),
    }
}

/// The same as [`atomic_write`], but for raw bytes (screenshots, previews).
///
/// The permissions of an already existing file are kept: replacing the name
/// creates a NEW file, and without this a config the customer locked down
/// (`0600`) would become world-readable after the very first save.
pub fn atomic_write_bytes(path: &str, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let target = atomic_target(Path::new(path));
    let target = target.as_ref();
    let name = target
        .file_name()
        .ok_or_else(|| std::io::Error::other("у пути нет имени файла"))?
        .to_string_lossy()
        .into_owned();
    let tmp = target.with_file_name(atomic_tmp_name(&name));
    let keep_mode = fs::metadata(target).ok().map(|m| m.permissions().mode());
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        if let Some(mode) = keep_mode {
            f.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        f.sync_all()?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// [`atomic_write`] with the error message as a string — the form for
/// executors whose contract is `Result<(), String>`.
pub fn atomic_write_str_err(path: &str, text: &str) -> Result<(), String> {
    atomic_write(path, text).map_err(|e| e.to_string())
}

/// An environment variable if it is set and NOT empty (empty = not set — that
/// is how the XDG specification treats it).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `os.path.expanduser` adjusted for XDG: `~` → `home`, the `~/.config/…` and
/// `~/.local/share/…` branches go to [`config_home`] and [`data_home`], the
/// rest expands as usual.
///
/// A compatible wrapper over [`expand_user_xdg`] without `$XDG_DATA_HOME`
/// (i.e. `~/.local/share` = `{home}/.local/share` — exactly what came out
/// before the data branch appeared).
pub fn expand_user(path: &str, home: &str, xdg_config_home: Option<&str>) -> String {
    expand_user_xdg(path, home, xdg_config_home, None)
}

/// The full expander: both XDG branches.
///
/// ⚠️ Prefix boundaries are checked EXACTLY (`~/.config` whole or
/// `~/.config/…`). The original had `startswith("~/.config")`, and real
/// neighbouring paths were cut in the middle: `~/.config-old/x` turned into
/// `~/.config/-old/x`, and `~/.local/shareX` — into `~/.local/share/X`. Rule
/// #16: a bug of the original is fixed in the port, not preserved.
pub fn expand_user_xdg(
    path: &str,
    home: &str,
    xdg_config_home: Option<&str>,
    xdg_data_home: Option<&str>,
) -> String {
    if path == "~" {
        let h = norm_dir(home);
        return if h.is_empty() { "/".to_string() } else { h.to_string() };
    }
    let Some(rest) = path.strip_prefix("~/") else {
        // Python returns anything not starting with `~/` as is (except
        // `~name`, which the core cannot handle — see [`expand_user_checked`]).
        return path.to_string();
    };
    if let Some(tail) = xdg_tail(rest, ".config") {
        let base = config_home(xdg_config_home, home);
        return if tail.is_empty() { base } else { join_path(&base, tail) };
    }
    if let Some(tail) = xdg_tail(rest, ".local/share") {
        let base = data_home(xdg_data_home, home);
        return if tail.is_empty() { base } else { join_path(&base, tail) };
    }
    join_path(norm_dir(home), rest)
}

/// The tail after an XDG prefix, if the path really lies under it.
///
/// `Some("")` — the directory itself, `Some("x/y")` — nested, `None` — the
/// prefix did not match at a BOUNDARY (`.config-old` ≠ `.config`).
fn xdg_tail<'a>(rest: &'a str, prefix: &str) -> Option<&'a str> {
    if rest == prefix {
        return Some("");
    }
    rest.strip_prefix(prefix)
        .filter(|t| t.starts_with('/'))
        .map(|t| t.trim_start_matches('/'))
}

/// Expander for the bridge: `None` — «the core is not responsible for this,
/// decide yourself».
///
/// Returns `None` on paths like `~name` (needs `pwd`, which the core does not
/// have) and with an unknown/empty `$HOME`. So live Python goes to
/// `os.path.expanduser` in these cases and the result cannot diverge from the
/// original.
pub fn expand_user_checked(
    path: &str,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
    xdg_data_home: Option<&str>,
) -> Option<String> {
    let home = home.filter(|h| !h.is_empty())?;
    if path.starts_with('~') && path != "~" && !path.starts_with("~/") {
        return None;
    }
    Some(expand_user_xdg(path, home, xdg_config_home, xdg_data_home))
}

/// Root of program state: `$XDG_STATE_HOME` or `~/.local/state`.
///
/// The fourth next to [`config_home`]/[`data_home`]/[`cache_home`]. The log is
/// state, not a setting: per the XDG specification it belongs here, and when
/// settings are moved to another machine it does not travel with them.
pub fn state_home(xdg_state_home: Option<&str>, home: &str) -> String {
    match xdg_state_home {
        Some(v) if !v.is_empty() => norm_dir(v).to_string(),
        _ => join_path(norm_dir(home), ".local/state"),
    }
}

/// Log directory: `<state>/rexon`.
pub fn log_dir(home: &str) -> String {
    log_dir_xdg(None, home)
}

/// Log directory respecting `$XDG_STATE_HOME`.
///
/// ⚠️ Until 08.10.2026 the log lived in `~/.config/rexon/rexon.log` and grew
/// without limit (1.5 MB a month at the owner's). The old file is neither
/// moved nor deleted: it is already someone else's history, not our working
/// file.
pub fn log_dir_xdg(xdg_state_home: Option<&str>, home: &str) -> String {
    join_path(&state_home(xdg_state_home, home), REXON_DIR_NAME)
}

/// Log file (`<state>/rexon/rexon.log`) — without looking at the environment.
pub fn log_file(home: &str) -> String {
    join_path(&log_dir(home), "rexon.log")
}

/// Log file from the process ENVIRONMENT — the only path `crate::applog`
/// writes to.
pub fn log_file_env() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let home = if home.is_empty() { ".".to_string() } else { home };
    join_path(
        &log_dir_xdg(env_nonempty("XDG_STATE_HOME").as_deref(), &home),
        "rexon.log",
    )
}

// ============================================================================
// Running external commands
// ============================================================================

/// Default timeout of an external command, seconds — the «quick query» class.
///
/// ⚠️ The name was kept, but its meaning is now narrow: this is the default
/// ONLY for quick commands. There is no single default for all calls anymore
/// — see `default_timeout_secs`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Quick query: `pacman -Q`, `systemctl is-enabled`, `gsettings get`,
/// `xdg-mime query`. Seconds. Did not answer in half a minute — something is wrong.
pub const QUICK_TIMEOUT_SECS: u64 = 30;

/// Downloading and installing from repositories: depends on the network and
/// the size of the package set. Half an hour — with a margin for a slow
/// mirror and a full desktop.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 1800;

/// Building from source (AUR, Gentoo): tens of minutes and entirely dependent
/// on the machine. Four hours is not «as long as needed» but «waiting longer
/// is pointless even on a weak machine»: it is protection against a process
/// that is stuck for good, not an estimate of the build time.
pub const BUILD_TIMEOUT_SECS: u64 = 14400;

/// Run-time class of an external command.
///
/// WHY. There was one default for the whole project — 30 seconds — and the
/// build of nine AUR packages was measured with it too. Installing the MATE
/// desktop on 2026-09-01 failed entirely: `yay` was killed after half a
/// minute, the customer read «AUR packages were not installed (9 of 9)» and
/// knew nothing about time. There are exactly three classes, and they are
/// told apart not by the caller's name but by WHAT the command does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunDuration {
    /// State query: answers right away or does not answer at all.
    Quick,
    /// Downloading and installing ready packages: minutes, depends on the network.
    Download,
    /// Building from source: tens of minutes, depends on the machine.
    Build,
}

impl RunDuration {
    /// Class timeout, seconds.
    pub fn timeout_secs(self) -> u64 {
        match self {
            RunDuration::Quick => QUICK_TIMEOUT_SECS,
            RunDuration::Download => DOWNLOAD_TIMEOUT_SECS,
            RunDuration::Build => BUILD_TIMEOUT_SECS,
        }
    }

    /// Short class name — for the log and for the bridge to Python.
    pub fn as_str(self) -> &'static str {
        match self {
            RunDuration::Quick => "quick",
            RunDuration::Download => "download",
            RunDuration::Build => "build",
        }
    }
}

/// Wrappers the real command hides behind: we strip them before parsing.
/// `env` can also take `VAR=VAL` before the name — those are skipped too.
pub const RUN_WRAPPERS: &[&str] = &["pkexec", "sudo", "doas", "env", "nice", "ionice", "stdbuf"];

/// Tools that BUILD from source: AUR helpers, makepkg, portage. The run time
/// is set by the machine, not the network.
pub const BUILD_TOOLS: &[&str] = &[
    "yay", "paru", "trizen", "pikaur", "aurman", "aura", "pacaur", "makepkg", "pamac-build",
    "emerge", "nix-env", "nix",
];

/// Package managers of ready packages and their subcommands that change the
/// system. Everything else of theirs (`search`, `info`, `list`, `-Q`) is a
/// quick query.
pub const DOWNLOAD_TOOLS: &[(&str, &[&str])] = &[
    ("apt", &["install", "upgrade", "full-upgrade", "dist-upgrade", "remove", "purge", "build-dep", "update", "reinstall"]),
    ("apt-get", &["install", "upgrade", "full-upgrade", "dist-upgrade", "remove", "purge", "build-dep", "update", "reinstall"]),
    ("dnf", &["install", "upgrade", "update", "remove", "reinstall", "group", "groupinstall", "distro-sync"]),
    ("yum", &["install", "upgrade", "update", "remove", "reinstall", "groupinstall"]),
    ("zypper", &["install", "in", "update", "up", "dup", "remove", "rm", "refresh", "ref", "patch"]),
    ("xbps-install", &[]),
    ("xbps-remove", &[]),
    ("apk", &["add", "del", "upgrade", "update", "fix"]),
    ("eopkg", &["install", "it", "remove", "rm", "upgrade", "up", "update-repo", "ur"]),
    ("swupd", &["bundle-add", "bundle-remove", "update"]),
    ("flatpak", &["install", "update", "uninstall"]),
    ("snap", &["install", "refresh", "remove"]),
    ("rpm-ostree", &["install", "upgrade", "uninstall", "update"]),
    ("pamac", &["install", "update", "remove", "upgrade"]),
    ("distrobox-create", &[]),
    ("git", &["clone", "fetch", "pull", "submodule"]),
    ("curl", &[]),
    ("wget", &[]),
];

/// Path fragments of our privileged wrappers: behind `pkexec bash
/// /tmp/rexon_install_*.sh` hides the installation of a whole desktop, and by
/// argv it could not be told apart from any other script otherwise.
pub const REXON_INSTALL_MARKERS: &[&str] =
    &["rexon-de-helper", "rexon_install_", "rexon_steamos_", "rexon-stage-helper"];

/// Strip wrappers (`pkexec`, `sudo`, `env VAR=VAL`, …) — return the index of
/// the first argument of the real command.
fn strip_wrappers(argv: &[String]) -> usize {
    let mut i = 0;
    while i < argv.len() {
        let base = basename(&argv[i]);
        if !RUN_WRAPPERS.contains(&base) {
            break;
        }
        i += 1;
        // `env`/`sudo` allow assignments before the command name.
        while i < argv.len() && argv[i].contains('=') && !argv[i].starts_with('-') {
            i += 1;
        }
        // Flags of the wrapper itself (`nice -n 19`, `sudo -u user`) are skipped
        // together with their value — otherwise we would take a flag for the command name.
        while i < argv.len() && argv[i].starts_with('-') {
            i += 1;
        }
    }
    i
}

/// The last path component — the executable's name.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `pacman` changes the system (install/remove/upgrade) — as opposed
/// to a query (`-Q…`, `-Ss`, `-Si`, `-T`, `-F`).
fn pacman_is_mutating(args: &[String]) -> bool {
    for a in args {
        if let Some(long) = a.strip_prefix("--") {
            match long {
                "sync" | "upgrade" | "remove" | "database" => return true,
                "query" | "search" | "info" | "deptest" | "files" | "version" | "help" => {
                    return false
                }
                _ => continue,
            }
        }
        let Some(letters) = a.strip_prefix('-') else {
            continue;
        };
        let mut chars = letters.chars();
        let Some(op) = chars.next() else { continue };
        match op {
            'Q' | 'T' | 'F' | 'V' | 'h' => return false,
            'S' => {
                // `-Ss`/`-Si`/`-Sg`/`-Sl`/`-Sp` are queries, not installation.
                return !letters[1..].chars().any(|c| matches!(c, 's' | 'i' | 'g' | 'l' | 'p'));
            }
            'U' | 'R' | 'D' => return true,
            _ => continue,
        }
    }
    false
}

/// Only a state query — neither installation nor building.
fn is_query_only(args: &[String]) -> bool {
    args.iter().any(|a| {
        a.starts_with("-Q")
            || matches!(
                a.as_str(),
                "-Ss" | "-Si" | "-T" | "-F" | "-V" | "-h"
                    | "--query" | "--search" | "--info" | "--version" | "--help"
            )
    })
}

/// Run-time class of a command by its argv.
///
/// Parsing goes by the MEANING of the command, not by who called it: the same
/// `pacman -Q` is quick both in a conformance check and in a desktop install,
/// while `yay -S` is long everywhere.
pub fn run_duration(argv: &[String]) -> RunDuration {
    if argv.is_empty() {
        return RunDuration::Quick;
    }
    // Our privileged wrappers: the script name says what is inside.
    if argv
        .iter()
        .any(|a| REXON_INSTALL_MARKERS.iter().any(|m| a.contains(m)))
    {
        return RunDuration::Download;
    }
    let start = strip_wrappers(argv);
    let Some(head) = argv.get(start) else {
        return RunDuration::Quick;
    };
    let tool = basename(head);
    let rest = &argv[(start + 1).min(argv.len())..];

    if BUILD_TOOLS.contains(&tool) {
        // An AUR helper can also query: `yay -Q`, `yay -Ss`. Everything
        // else it does is building, and assuming otherwise is not allowed: an
        // error in this direction kills the build by timeout, an error in the
        // other direction only waits longer for a hung query.
        return if is_query_only(rest) {
            RunDuration::Quick
        } else {
            RunDuration::Build
        };
    }
    if tool == "pacman" || tool == "pacman-key" {
        return if pacman_is_mutating(rest) {
            RunDuration::Download
        } else {
            RunDuration::Quick
        };
    }
    for (name, subcommands) in DOWNLOAD_TOOLS {
        if tool != *name {
            continue;
        }
        if subcommands.is_empty() {
            return RunDuration::Download;
        }
        if rest
            .iter()
            .any(|a| subcommands.contains(&a.as_str()))
        {
            return RunDuration::Download;
        }
        return RunDuration::Quick;
    }
    RunDuration::Quick
}

/// Default timeout for a command, seconds — when the caller did not set its
/// own.
pub fn default_timeout_secs(argv: &[String]) -> u64 {
    run_duration(argv).timeout_secs()
}

/// What has to be added to an external command call beyond what was passed.
///
/// Capturing output without an explicit encoding gives bytes and breaks on
/// non-Latin text in package manager output, so when capturing, text mode
/// with replacement of invalid bytes is always enforced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunDefaults {
    /// Enable text mode with utf-8 and replacement of invalid bytes.
    pub text_mode: bool,
    /// Timeout, if the caller did not set one.
    pub timeout_secs: Option<u64>,
}

/// Additions to an external command call.
///
/// The timeout is filled in BY THE COMMAND'S CLASS (`run_duration`), not one
/// for all: the same default that measures `pacman -Q` cannot measure an AUR
/// build.
pub fn run_defaults(argv: &[String], has_capture_output: bool, has_timeout: bool) -> RunDefaults {
    RunDefaults {
        text_mode: has_capture_output,
        timeout_secs: if has_timeout {
            None
        } else {
            Some(default_timeout_secs(argv))
        },
    }
}

/// Command timeout message.
pub fn msg_timeout(command: &str, timeout: &str) -> String {
    // TRANSLATORS: %1 is a command, %2 the time limit in seconds.
    crate::i18n::trf("Command %1: timed out (%2 s)", &[&command, &timeout])
}

/// Launch error message.
pub fn msg_run_failed(command: &str, error: &str) -> String {
    // TRANSLATORS: %1 is a command, %2 the system's error text.
    crate::i18n::trf("Couldn't run the command %1: %2", &[&command, &error])
}

// ============================================================================
// External tools — the SINGLE lookup point for the whole project
// ============================================================================
//
// Rule #12 (fix the engine, not the cases). One and the same class of defect
// was caught three times: code calls a binary by ONE name, while on the
// target distribution it is called differently or lies outside PATH — and
// the failure is silent: a line goes to the log, and the UI reports success.
//
//   • `qdbus` — no target system has such a name (qdbus6 / qdbus-qt6), and
//     on Kubuntu it is also outside PATH (Launchpad #2084278);
//   • `kwriteconfig6`/`kreadconfig6` — on Ubuntu/Kubuntu 24.04 LTS (Plasma 5)
//     they do not exist at all, only `*5`: the theme, colors, fonts, cursor,
//     KSplash, lock screen wallpaper and KWin rules were silently not applied
//     there;
//   • `swww` — upstream renamed the project to `awww` (Oct 2025), the package
//     ships only `awww`/`awww-daemon`.
//
// So binary names go through ONE resolver: iterating over synonyms → PATH →
// directories outside PATH → one clear message with the package name for the
// CURRENT distribution.

/// Synonym sets: canonical name → (candidates, directories outside PATH).
///
/// The canonical name is the one calling modules write in argv. The order of
/// candidates matters: the first one found is taken.
pub const TOOL_SETS: &[(&str, &[&str], &[&str])] = &[
    // KDE Frameworks: 6 → 5 (Ubuntu/Kubuntu 24.04 LTS = Plasma 5)
    ("kwriteconfig6", &["kwriteconfig6", "kwriteconfig5"], &[]),
    ("kreadconfig6", &["kreadconfig6", "kreadconfig5"], &[]),
    ("kpackagetool6", &["kpackagetool6", "kpackagetool5"], &[]),
    ("kbuildsycoca6", &["kbuildsycoca6", "kbuildsycoca5"], &[]),
    ("kquitapp6", &["kquitapp6", "kquitapp5"], &[]),
    // Launching a program detached from the caller: in Plasma 6 the binary is
    // called `kstart`, in Plasma 5 — `kstart5`.
    ("kstart", &["kstart", "kstart5"], &[]),
    ("kdialog", &["kdialog"], &[]),
    // Wayland wallpapers: swww was renamed to awww, the arguments are compatible
    ("swww", &["swww", "awww"], &[]),
    ("swww-daemon", &["swww-daemon", "awww-daemon"], &[]),
    // ImageMagick: on IM7 `magick` is a drop-in replacement for `convert`,
    // on Ubuntu 24.04 (IM6) conversely only `convert` exists
    ("convert", &["convert", "magick"], &[]),
    // 7-Zip: the `7zip` package ships `7zz` (Debian/Ubuntu), the old p7zip — `7za`;
    // the `x -o<dir>` arguments are the same for all three
    ("7z", &["7z", "7zz", "7za"], &[]),
    // Plymouth: on Debian/openSUSE the binary lies in /usr/sbin, which is not
    // in a regular user's PATH
    (
        "plymouth-set-default-theme",
        &["plymouth-set-default-theme"],
        &["/usr/sbin", "/sbin"],
    ),
    (
        "update-alternatives",
        &["update-alternatives"],
        &["/usr/sbin", "/sbin"],
    ),
    (
        "update-initramfs",
        &["update-initramfs"],
        &["/usr/sbin", "/sbin"],
    ),
    // ⚠️ pactl HAS NO SYNONYMS. wpctl (wireplumber) solves the same task,
    // but it has a different command set and different output — argv[0]
    // must not be swapped, that would give garbage instead of a device
    // list. The wpctl branch is in `audio_capture::wpctl_*`, not a name
    // substitution.
    // Qt D-Bus client: its name differs on each distribution, and on
    // Kubuntu it is also outside PATH.
    (
        "qdbus6",
        &["qdbus6", "qdbus-qt6", "qdbus", "qdbus-qt5"],
        &[
            "/usr/lib/qt6/bin",
            "/usr/lib64/qt6/bin",
            "/usr/lib/*/qt6/bin",
        ],
    ),
];

/// Tools that may have no binary of their own at all — only a subcommand of a
/// shared one.
///
/// ImageMagick 7 is gradually removing separate binaries: `convert` still
/// lives in the synonym set above (there it is ONE word: `magick` is a drop-in
/// replacement), while `identify` in IM7 exists as `magick identify`, i.e. TWO
/// words, and swapping argv[0] does not cure that. The value is the full
/// command prefix.
pub const TOOL_SUBCOMMANDS: &[(&str, &[&str])] = &[("identify", &["magick", "identify"])];

/// Package name by binary and package manager family.
///
/// So that both in the log and in the UI (rule #13) we tell the person WHAT to
/// install, not «command not found». Family keys are `system::PkgFamily`
/// values. The order of families inside an entry matters: for an unknown
/// family `debian` is taken, and if that is missing too — the FIRST value of
/// the entry.
pub const TOOL_PACKAGES: &[(&str, &[(&str, &str)])] = &[
    // The compositor for «Screen effects» on MATE, Xfce and LXQt
    // (`screen_effects::install_packages`).
    (
        "picom",
        &[
            ("arch", "picom"),
            ("fedora", "picom"),
            ("suse", "picom"),
            ("debian", "picom"),
        ],
    ),
    (
        "convert",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    (
        "magick",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    (
        "identify",
        &[
            ("arch", "imagemagick"),
            ("fedora", "ImageMagick"),
            ("suse", "ImageMagick"),
            ("debian", "imagemagick"),
        ],
    ),
    // bsdtar (libarchive) unpacks 7z too. Names checked against the
    // repositories 19.10.2026: Arch — in libarchive (pacman depends on it),
    // Debian/Ubuntu — libarchive-tools, Fedora and openSUSE — a separate bsdtar.
    (
        "bsdtar",
        &[
            ("arch", "libarchive"),
            ("fedora", "bsdtar"),
            ("suse", "bsdtar"),
            ("debian", "libarchive-tools"),
        ],
    ),
    // Invisible X server for Conky widget snapshots (`conky_preview`) — a
    // mandatory dependency of the Rexon package; on SteamOS — an Arch package
    // inside the Rexon sysext. Names are the distributions' official packages.
    (
        "Xvfb",
        &[
            ("arch", "xorg-server-xvfb"),
            ("fedora", "xorg-x11-server-Xvfb"),
            ("suse", "xorg-x11-server-Xvfb"),
            ("debian", "xvfb"),
        ],
    ),
    (
        "7z",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "7zz",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "wrestool",
        &[
            ("arch", "icoutils"),
            ("fedora", "icoutils"),
            ("suse", "icoutils"),
            ("debian", "icoutils"),
        ],
    ),
    (
        "icotool",
        &[
            ("arch", "icoutils"),
            ("fedora", "icoutils"),
            ("suse", "icoutils"),
            ("debian", "icoutils"),
        ],
    ),
    (
        "xcursorgen",
        &[
            ("arch", "xorg-xcursorgen"),
            ("fedora", "xcursorgen"),
            ("suse", "xcursorgen"),
            ("debian", "x11-apps"),
        ],
    ),
    (
        "cabextract",
        &[
            ("arch", "cabextract"),
            ("fedora", "cabextract"),
            ("suse", "cabextract"),
            ("debian", "cabextract"),
        ],
    ),
    // Qt style switcher for environments without their own (everything except
    // KDE and LXQt): gives Qt programs the Kvantum style together with fonts
    // and icons. Names checked against repology and distribution repositories;
    // Gentoo keeps the name with the category, like the desktops in `_DE_DATABASE`.
    (
        "qt6ct",
        &[
            ("arch", "qt6ct"),
            ("fedora", "qt6ct"),
            ("suse", "qt6ct"),
            ("debian", "qt6ct"),
            ("gentoo", "gui-apps/qt6ct"),
            ("alpine", "qt6ct"),
            ("void", "qt6ct"),
            ("solus", "qt6ct"),
            ("alt", "qt6ct"),
            // Clear Linux has no bundle with qt6ct, in NixOS packages are
            // installed by editing the configuration — there the fallback via
            // QT_STYLE_OVERRIDE works, nothing needs to be delivered.
            ("clear", "—"),
            ("nix", "—"),
        ],
    ),
    // No package of its own anywhere except the AUR: everywhere else it stays an install from PyPI
    (
        "win2xcur",
        &[
            ("arch", "win2xcur (из AUR)"),
            ("fedora", "win2xcur (pip install win2xcur)"),
            ("suse", "win2xcur (pip install win2xcur)"),
            ("debian", "win2xcur (pip install win2xcur)"),
        ],
    ),
    (
        "ffmpeg",
        &[
            ("arch", "ffmpeg"),
            ("fedora", "ffmpeg-free"),
            ("suse", "ffmpeg-7"),
            ("debian", "ffmpeg"),
        ],
    ),
    (
        "7za",
        &[
            ("arch", "7zip"),
            ("fedora", "7zip"),
            ("suse", "7zip"),
            ("debian", "7zip"),
        ],
    ),
    (
        "notify-send",
        &[
            ("arch", "libnotify"),
            ("fedora", "libnotify"),
            ("suse", "libnotify-tools"),
            ("debian", "libnotify-bin"),
        ],
    ),
    // pw-play/pw-cat: Fedora and openSUSE moved them out of the main package
    (
        "pw-play",
        &[
            ("arch", "pipewire"),
            ("fedora", "pipewire-utils"),
            ("suse", "pipewire-tools"),
            ("debian", "pipewire-bin"),
        ],
    ),
    (
        "paplay",
        &[
            ("arch", "libpulse"),
            ("fedora", "pulseaudio-utils"),
            ("suse", "pulseaudio-utils"),
            ("debian", "pulseaudio-utils"),
        ],
    ),
    (
        "aplay",
        &[
            ("arch", "alsa-utils"),
            ("fedora", "alsa-utils"),
            ("suse", "alsa-utils"),
            ("debian", "alsa-utils"),
        ],
    ),
    (
        "canberra-gtk-play",
        &[
            ("arch", "libcanberra"),
            ("fedora", "libcanberra-gtk3"),
            ("suse", "libcanberra-gtk3"),
            ("debian", "gnome-session-canberra"),
        ],
    ),
    (
        "glib-compile-schemas",
        &[
            ("arch", "glib2"),
            ("fedora", "glib2"),
            ("suse", "glib2-tools"),
            ("debian", "libglib2.0-bin"),
        ],
    ),
    (
        "dbus-send",
        &[
            ("arch", "dbus"),
            ("fedora", "dbus-tools"),
            ("suse", "dbus-1-tools"),
            ("debian", "dbus-bin"),
        ],
    ),
    (
        "pactl",
        &[
            ("arch", "libpulse"),
            ("fedora", "pulseaudio-utils"),
            ("suse", "pulseaudio-utils"),
            ("debian", "pulseaudio-utils"),
        ],
    ),
    // Present wherever PipeWire is: wireplumber is a mandatory part of the stack.
    (
        "wpctl",
        &[
            ("arch", "wireplumber"),
            ("fedora", "wireplumber"),
            ("suse", "wireplumber"),
            ("debian", "wireplumber"),
        ],
    ),
    // NOT in the official Arch repositories (AUR only); in Fedora — since 42;
    // Debian/Ubuntu/openSUSE have it, but it is installed neither by default
    // nor by dependencies — it is not there «out of the box» anywhere.
    (
        "xdg-terminal-exec",
        &[
            ("arch", "xdg-terminal-exec (из AUR)"),
            ("fedora", "xdg-terminal-exec (Fedora 42+)"),
            ("suse", "xdg-terminal-exec"),
            ("debian", "xdg-terminal-exec"),
        ],
    ),
    // On openSUSE pkexec is a separate subpackage, pulled in only via Recommends
    (
        "pkexec",
        &[
            ("arch", "polkit"),
            ("fedora", "polkit"),
            ("suse", "polkit"),
            ("debian", "pkexec"),
        ],
    ),
    (
        "xprop",
        &[
            ("arch", "xorg-xprop"),
            ("fedora", "xprop"),
            ("suse", "xprop"),
            ("debian", "x11-utils"),
        ],
    ),
    (
        "xrandr",
        &[
            ("arch", "xorg-xrandr"),
            ("fedora", "xrandr"),
            ("suse", "xrandr"),
            ("debian", "x11-xserver-utils"),
        ],
    ),
    (
        "git",
        &[
            ("arch", "git"),
            ("fedora", "git-core"),
            ("suse", "git-core"),
            ("debian", "git"),
        ],
    ),
    (
        "kvantummanager",
        &[
            ("arch", "kvantum"),
            ("fedora", "kvantum"),
            ("suse", "kvantum-manager"),
            ("debian", "qt6-style-kvantum"),
        ],
    ),
    (
        "cvlc",
        &[
            ("arch", "vlc-cli"),
            ("fedora", "vlc-cli"),
            ("suse", "vlc-noX"),
            ("debian", "vlc-bin"),
        ],
    ),
    (
        "qdbus6",
        &[
            ("arch", "qt6-tools"),
            ("fedora", "qt6-qttools"),
            ("suse", "qt6-tools-qdbus"),
            ("debian", "qdbus-qt6"),
        ],
    ),
    (
        "kwriteconfig6",
        &[
            ("arch", "kconfig"),
            ("fedora", "kf6-kconfig"),
            ("suse", "kf6-kconfig"),
            ("debian", "libkf6config-bin"),
        ],
    ),
    (
        "kreadconfig6",
        &[
            ("arch", "kconfig"),
            ("fedora", "kf6-kconfig"),
            ("suse", "kf6-kconfig"),
            ("debian", "libkf6config-bin"),
        ],
    ),
    (
        "swww",
        &[
            ("arch", "awww"),
            ("fedora", "awww"),
            ("suse", "awww"),
            ("debian", "awww"),
        ],
    ),
    (
        "mpvpaper",
        &[
            ("arch", "mpvpaper"),
            ("fedora", "mpvpaper (COPR)"),
            ("suse", "mpvpaper"),
            ("debian", "—"),
        ],
    ),
    (
        "yt-dlp",
        &[
            ("arch", "yt-dlp"),
            ("fedora", "yt-dlp"),
            ("suse", "yt-dlp"),
            ("debian", "yt-dlp"),
        ],
    ),
    (
        "xwinwrap",
        &[
            ("arch", "xwinwrap-git (из AUR)"),
            ("fedora", "—"),
            ("suse", "—"),
            ("debian", "—"),
        ],
    ),
    (
        "mpv",
        &[
            ("arch", "mpv"),
            ("fedora", "mpv"),
            ("suse", "mpv"),
            ("debian", "mpv"),
        ],
    ),
    (
        "hyprpaper",
        &[
            ("arch", "hyprpaper"),
            ("fedora", "hyprpaper"),
            ("suse", "hyprpaper"),
            ("debian", "—"),
        ],
    ),
    (
        "ruffle",
        &[
            ("arch", "ruffle (из AUR)"),
            ("fedora", "—"),
            ("suse", "ruffle"),
            ("debian", "—"),
        ],
    ),
    (
        "gtk-update-icon-cache",
        &[
            ("arch", "gtk-update-icon-cache"),
            ("fedora", "gtk-update-icon-cache"),
            ("suse", "gtk3-tools"),
            ("debian", "libgtk-3-bin"),
        ],
    ),
    (
        "update-desktop-database",
        &[
            ("arch", "desktop-file-utils"),
            ("fedora", "desktop-file-utils"),
            ("suse", "desktop-file-utils"),
            ("debian", "desktop-file-utils"),
        ],
    ),
    // Programs Conky widgets call most often (`${exec sensors}`, weather
    // via wget/curl, a player) — the widget health check
    // (`conky_safety::health`) installs their package before launch. Sensors:
    // lm_sensors in Arch and Fedora, lm-sensors in Debian/Ubuntu, sensors in
    // openSUSE. wget in Fedora since release 40 is wget2, and the former
    // program name comes from the wget2-wget package. nvidia-smi is
    // deliberately not here: it comes with the NVIDIA driver, and it must not
    // be installed as a separate package.
    (
        "sensors",
        &[
            ("arch", "lm_sensors"),
            ("fedora", "lm_sensors"),
            ("suse", "sensors"),
            ("debian", "lm-sensors"),
        ],
    ),
    (
        "wget",
        &[
            ("arch", "wget"),
            ("fedora", "wget2-wget"),
            ("suse", "wget"),
            ("debian", "wget"),
        ],
    ),
    (
        "curl",
        &[
            ("arch", "curl"),
            ("fedora", "curl"),
            ("suse", "curl"),
            ("debian", "curl"),
        ],
    ),
    (
        "jq",
        &[
            ("arch", "jq"),
            ("fedora", "jq"),
            ("suse", "jq"),
            ("debian", "jq"),
        ],
    ),
    // Audio visualizer of equalizer widgets (`io.popen('cava -p …')` in
    // Lua). Checked against the repositories 24.10.2026: cava is in Arch
    // (extra), Debian/Ubuntu, Fedora and openSUSE.
    (
        "cava",
        &[
            ("arch", "cava"),
            ("fedora", "cava"),
            ("suse", "cava"),
            ("debian", "cava"),
        ],
    ),
    (
        "bc",
        &[
            ("arch", "bc"),
            ("fedora", "bc"),
            ("suse", "bc"),
            ("debian", "bc"),
        ],
    ),
    (
        "playerctl",
        &[
            ("arch", "playerctl"),
            ("fedora", "playerctl"),
            ("suse", "playerctl"),
            ("debian", "playerctl"),
        ],
    ),
];

/// Candidates and directories outside PATH for a tool.
///
/// An unknown name is its own single candidate (exactly like
/// `_TOOL_SETS.get(tool, ((tool,), ()))` in the original).
pub fn tool_candidates(tool: &str) -> (Vec<&str>, Vec<&str>) {
    for (name, names, dirs) in TOOL_SETS {
        if *name == tool {
            return (names.to_vec(), dirs.to_vec());
        }
    }
    (vec![tool], Vec::new())
}

/// Multi-word prefix of a subcommand tool (`identify` → `magick identify`),
/// if it is described.
pub fn tool_subcommand(tool: &str) -> Option<&'static [&'static str]> {
    TOOL_SUBCOMMANDS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, prefix)| *prefix)
}

/// Whether substitution variants are known for the name — what
/// `resolved_argv` checks before searching for anything at all.
pub fn tool_has_variants(tool: &str) -> bool {
    TOOL_SETS.iter().any(|(name, _, _)| *name == tool) || tool_subcommand(tool).is_some()
}

/// Name of the package with this tool for the given family, or `""`.
///
/// The single source of package names in the UI and in the log (rule #13: the
/// customer must not be advised `pacman -S` on Fedora). The family comes as a
/// parameter: it is determined by `system::DistroDetector`, not by this table.
pub fn tool_package_name(binary: &str, family: &str) -> String {
    let Some((_, table)) = TOOL_PACKAGES.iter().find(|(name, _)| *name == binary) else {
        return String::new();
    };
    let lookup = |key: &str| {
        table
            .iter()
            .find(|(f, _)| *f == key)
            .map(|(_, pkg)| *pkg)
            .filter(|pkg| !pkg.is_empty())
    };
    // The family is not in the table — name the package the way it is
    // called on most distributions, but without an install command.
    let pkg = lookup(family)
        .or_else(|| lookup("debian"))
        .or_else(|| table.first().map(|(_, pkg)| *pkg))
        .unwrap_or("");
    // «—» in the table means «there is no package at all» — nothing to advise.
    if pkg.is_empty() || pkg == "—" {
        String::new()
    } else if let Some(name) = pkg.strip_suffix(AUR_ONLY_MARK) {
        // The table mark is an identifier, the label is in the interface
        // language. The bracket stays at the start of the mark: by it
        // `dep_package_name` cuts off the name.
        // TRANSLATORS: %1 is a package that exists only in the AUR (Arch User Repository).
        crate::i18n::trf("%1 (from the AUR)", &[&name])
    } else {
        pkg.to_string()
    }
}

/// The «package only in the AUR» mark in the [`TOOL_PACKAGES`] table is an
/// identifier, not a label: [`tool_package_name`] shows it in the interface
/// language.
pub(crate) const AUR_ONLY_MARK: &str = " (из AUR)";

/// All packages of the [`TOOL_PACKAGES`] table for the family — both
/// repository ones and those marked «AUR only» (without the mark). The
/// install allowlist of the package helper
/// (`system_plan::pkg_helper_install_allowlist`): Conky widget programs and
/// the 7z unpacker are installed only from here.
pub fn tool_packages_for(family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (_, table) in TOOL_PACKAGES {
        for (f, pkg) in table.iter() {
            if *f != family {
                continue;
            }
            let pkg = pkg.strip_suffix(AUR_ONLY_MARK).unwrap_or(pkg);
            if !pkg.is_empty() && pkg != "—" && !pkg.contains('(') && !out.contains(&pkg) {
                out.push(pkg);
            }
        }
    }
    out
}

/// «Install the X package» for the given family, or `""`.
pub fn tool_package_hint(binary: &str, family: &str) -> String {
    let pkg = tool_package_name(binary, family);
    if pkg.is_empty() {
        String::new()
    } else {
        // TRANSLATORS: %1 is a package name; the sentence tells the user what to install.
        crate::i18n::trf("Install the “%1” package", &[&pkg])
    }
}

// ----------------------------------------------------------------------------
// Playing sound: ONE player choice for the whole project
// ----------------------------------------------------------------------------
//
// ⚠️ Why it is here and not in place. The «paplay → ffplay → aplay →
// canberra-gtk-play» chain failed twice at once: `paplay` lives in
// `pulseaudio-utils`, which pure PipeWire systems (Ubuntu 24.04, Kubuntu
// 24.04, Debian 13) do not have, and `canberra-gtk-play` on Debian/Ubuntu
// lives not in `libcanberra-gtk*-module`, as dependencies usually say, but in
// `gnome-session-canberra`, which is not installed either. Plus `aplay`
// (recommended as «a reliable tail») understands ONLY WAV, and freedesktop
// sound themes consist of `.oga` — that is, the «tail» silently played
// nothing. Hence the table: for each player it lists WHAT it can do.

/// Sound file players: (binary, arguments before the path, extensions).
///
/// The order goes from «present wherever there is sound» to narrow ones. An
/// empty extension list means «can play everything».
pub const SOUND_PLAYERS: &[(&str, &[&str], &[&str])] = &[
    // pipewire: present wherever PipeWire is (that is, on all targets)
    ("pw-play", &[], &["oga", "ogg", "opus", "wav", "flac"]),
    // pulseaudio-utils: where the stack is still pulse
    ("paplay", &[], &["oga", "ogg", "opus", "wav", "flac"]),
    // libcanberra: plays a file with the -f key (not to be confused with the -i event)
    ("canberra-gtk-play", &["-f"], &["oga", "ogg", "wav"]),
    // ffmpeg: understands everything, but is not installed everywhere
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"], &[]),
    // alsa-utils: the last line of defense, but ONLY WAV
    ("aplay", &["-q"], &["wav"]),
];

/// Lowercase file extension, without the dot.
fn file_ext(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// argv for playing a FILE with the first suitable player.
///
/// `available` — binaries the executor found in PATH. `None` — nothing to
/// play with: no installed player understands this format.
pub fn sound_play_argv(available: &[String], file_path: &str) -> Option<Vec<String>> {
    let ext = file_ext(file_path);
    for (binary, args, exts) in SOUND_PLAYERS {
        if !available.iter().any(|b| b == binary) {
            continue;
        }
        if !exts.is_empty() && !exts.contains(&ext.as_str()) {
            continue;
        }
        let mut argv = vec![(*binary).to_string()];
        argv.extend(args.iter().map(|a| (*a).to_string()));
        argv.push(file_path.to_string());
        return Some(argv);
    }
    None
}

/// argv for playing a theme EVENT (`canberra-gtk-play -i`) — the path for
/// when no event file was found in the theme.
pub fn sound_play_event_argv(available: &[String], event_id: &str) -> Option<Vec<String>> {
    if available.iter().any(|b| b == "canberra-gtk-play") {
        return Some(vec![
            "canberra-gtk-play".to_string(),
            "-i".to_string(),
            event_id.to_string(),
        ]);
    }
    None
}

/// What to advise installing if there is nothing to play with: for WAV
/// `alsa-utils` is enough, freedesktop themes (`.oga`) need a real decoder.
pub fn sound_player_suggestion(file_path: &str) -> &'static str {
    if file_ext(file_path) == "wav" {
        "aplay"
    } else {
        "pw-play"
    }
}

/// UI message when there is nothing to play with (rule #13 — no raw binary
/// names at the start of the phrase). `hint` — «Install the … package» or empty.
pub fn msg_no_sound_player(hint: &str) -> String {
    if hint.is_empty() {
        crate::i18n::tr("Can't play the sound: no audio player is installed")
    } else {
        // TRANSLATORS: %1 is a hint like “Install the “pipewire” package”.
        crate::i18n::trf("Can't play the sound: no audio player is installed. %1", &[&hint])
    }
}

/// ONE clear log record instead of «command not found».
///
/// Assembled here so the text is the same both in the log and in the UI: the
/// name, all tried synonyms and the package for the current distribution.
pub fn msg_missing_tool(names: &[&str], hint: &str) -> String {
    // Whole phrases rather than glued pieces: the translator needs the whole
    // word order, and «(tried: …)» and the hint go elsewhere in another language.
    let tool = names.first().copied().unwrap_or("");
    let tried = names.join(", ");
    use crate::i18n::trf;
    match (names.len() > 1, hint.is_empty()) {
        // TRANSLATORS: %1 is a program name.
        (false, true) => trf("Tool %1 not found", &[&tool]),
        // TRANSLATORS: %1 is a program name, %2 the other names that were tried.
        (true, true) => trf("Tool %1 not found (tried: %2)", &[&tool, &tried]),
        // TRANSLATORS: %1 is a program name, %2 a hint like “Install the “x” package”.
        (false, false) => trf("Tool %1 not found. %2", &[&tool, &hint]),
        // TRANSLATORS: %1 is a program name, %2 the other names that were tried, %3 a hint like “Install the “x” package”.
        (true, false) => trf("Tool %1 not found (tried: %2). %3", &[&tool, &tried, &hint]),
    }
}

// ----------------------------------------------------------------------------
// Binary lookup
// ----------------------------------------------------------------------------

/// PATH when the variable does not exist at all: the same that
/// `confstr("CS_PATH")` returns on glibc and that `shutil.which` takes.
pub const CS_PATH_FALLBACK: &str = "/bin:/usr/bin";

/// Splitting a path like `os.path.split`: (directory, name).
fn path_split(p: &str) -> (&str, &str) {
    match p.rfind('/') {
        None => ("", p),
        Some(i) => {
            let (head, tail) = p.split_at(i + 1);
            // A directory of slashes only stays as is (`/ls` → `/`).
            let head = if head.chars().all(|c| c == '/') {
                head
            } else {
                head.trim_end_matches('/')
            };
            (head, tail)
        }
    }
}

/// Whether a file can be executed: it exists, is not a directory, has the `x` bit.
///
/// ⚠️ About `access(2)` — see the warning at the start of the module.
fn is_executable_file(path: &str) -> bool {
    match fs::metadata(path) {
        Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Path to a command via PATH — port of `shutil.which(cmd)`.
///
/// `path_env` — the `$PATH` value (`None` — no variable; an empty string, as
/// in Python, means «search nowhere»). A name with a slash is looked up only
/// in its own directory, PATH is not consulted then.
pub fn which(cmd: &str, path_env: Option<&str>) -> Option<String> {
    let (dirname, base) = path_split(cmd);
    let dirs: Vec<String> = if !dirname.is_empty() {
        vec![dirname.to_string()]
    } else {
        let path = path_env.unwrap_or(CS_PATH_FALLBACK);
        // PATH='' matches nothing, while PATH=':' looks into the current
        // directory — the same quirk as shutil.which.
        if path.is_empty() {
            return None;
        }
        path.split(':').map(str::to_string).collect()
    };
    let mut seen: HashSet<&str> = HashSet::new();
    for dir in &dirs {
        if !seen.insert(dir.as_str()) {
            continue;
        }
        let cand = join_path(dir, base);
        if is_executable_file(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Whether the pattern has something glob expands.
///
/// The core does not support the `[...]` character class (synonym sets have
/// no such patterns and none are planned) — a pattern with it counts as literal.
fn has_glob_magic(part: &str) -> bool {
    part.contains('*') || part.contains('?')
}

/// Matching a name against a pattern of `*` and `?`.
fn glob_match(name: &str, pattern: &str) -> bool {
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    // The classic two-dimensional pass: m[i][j] — «the first i characters of
    // the name are covered by the first j characters of the pattern».
    let mut prev = vec![false; p.len() + 1];
    prev[0] = true;
    for j in 1..=p.len() {
        prev[j] = prev[j - 1] && p[j - 1] == '*';
    }
    for i in 1..=n.len() {
        let mut cur = vec![false; p.len() + 1];
        for j in 1..=p.len() {
            cur[j] = match p[j - 1] {
                '*' => cur[j - 1] || prev[j],
                '?' => prev[j - 1],
                c => prev[j - 1] && n[i - 1] == c,
            };
        }
        prev = cur;
    }
    prev[p.len()]
}

/// Expanding a directory pattern together with a file name — port of
/// `glob.glob(os.path.join(pattern, name))`.
///
/// The result is NOT sorted (as in Python); the caller sorts.
fn glob_paths(pattern: &str) -> Vec<String> {
    if !pattern.split('/').any(has_glob_magic) {
        return if Path::new(pattern).exists() {
            vec![pattern.to_string()]
        } else {
            Vec::new()
        };
    }
    let absolute = pattern.starts_with('/');
    let parts: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur: Vec<String> = vec![if absolute { "/".into() } else { String::new() }];
    for (idx, part) in parts.iter().enumerate() {
        let last = idx + 1 == parts.len();
        let mut next = Vec::new();
        for dir in &cur {
            if has_glob_magic(part) {
                let read_root = if dir.is_empty() { "." } else { dir.as_str() };
                let Ok(entries) = fs::read_dir(read_root) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    // `*` does not catch hidden files — like glob.
                    if name.starts_with('.') && !part.starts_with('.') {
                        continue;
                    }
                    if !glob_match(&name, part) {
                        continue;
                    }
                    let full = join_path(dir, &name);
                    if last || Path::new(&full).is_dir() {
                        next.push(full);
                    }
                }
            } else {
                let full = join_path(dir, part);
                if if last {
                    Path::new(&full).exists()
                } else {
                    Path::new(&full).is_dir()
                } {
                    next.push(full);
                }
            }
        }
        cur = next;
    }
    cur
}

/// Binary lookup over all synonyms: PATH, then directories outside PATH.
///
/// Exactly the original's order: first all names via PATH, and only if none
/// was found — iterating over directories (the `*` pattern is expanded in
/// them, candidates are sorted, the first executable is taken).
pub fn lookup_tool(names: &[&str], dirs: &[&str], path_env: Option<&str>) -> Option<String> {
    for name in names {
        if let Some(exe) = which(name, path_env) {
            return Some(exe);
        }
    }
    // PATH did not help — look into directories outside PATH directly.
    for pattern in dirs {
        for name in names {
            let mut cands = glob_paths(&join_path(pattern, name));
            cands.sort();
            if let Some(exe) = cands.into_iter().find(|c| is_executable_file(c)) {
                return Some(exe);
            }
        }
    }
    None
}

/// Path to an external tool by its canonical name, or `None`.
///
/// No cache: the cache lives in the caller (in live Python —
/// `utils._tool_cache`, keyed by PATH), so there are not two sources of truth.
pub fn resolve_tool(tool: &str, extra_dirs: &[&str], path_env: Option<&str>) -> Option<String> {
    let (names, mut dirs) = tool_candidates(tool);
    dirs.extend_from_slice(extra_dirs);
    lookup_tool(&names, &dirs, path_env)
}

/// Ready argv of an external tool, or `None` if it does not exist at all.
///
/// Knows both synonyms (`convert` → `magick`) and multi-word variants
/// (`identify` → `magick identify`). The answer to «is the tool there» comes
/// from here too — so the check and the launch cannot diverge.
pub fn tool_argv(tool: &str, args: &[String], path_env: Option<&str>) -> Option<Vec<String>> {
    if let Some(exe) = resolve_tool(tool, &[], path_env) {
        let mut argv = vec![exe];
        argv.extend(args.iter().cloned());
        return Some(argv);
    }
    let prefix = tool_subcommand(tool)?;
    let exe = resolve_tool(prefix[0], &[], path_env)?;
    let mut argv = vec![exe];
    argv.extend(prefix[1..].iter().map(|s| (*s).to_string()));
    argv.extend(args.iter().cloned());
    Some(argv)
}

/// argv with an already resolved binary.
///
/// If the tool does not exist at all, argv[0] stays the canonical name — the
/// call will honestly fail, but the cause is already clear from the log. That
/// way the calling code does not have to change form.
pub fn tool_cmd(tool: &str, args: &[String], path_env: Option<&str>) -> Vec<String> {
    if let Some(argv) = tool_argv(tool, args, path_env) {
        return argv;
    }
    let mut argv = vec![tool.to_string()];
    argv.extend(args.iter().cloned());
    argv
}

/// argv with a substituted tool, if variants are known for it.
///
/// Everything not in the synonym sets passes straight through — zero
/// overhead. The substitution also applies to argv plans built by the core:
/// the executor launches the process anyway, and it calls this same resolver.
pub fn resolved_argv(cmd: &[String], path_env: Option<&str>) -> Vec<String> {
    let Some(head) = cmd.first() else {
        return cmd.to_vec();
    };
    if !tool_has_variants(head) {
        return cmd.to_vec();
    }
    match tool_argv(head, &[], path_env) {
        // Nothing changed — return the original so as not to multiply copies.
        Some(argv) if argv.len() == 1 && argv[0] == *head => cmd.to_vec(),
        Some(argv) => {
            let mut out = argv;
            out.extend(cmd[1..].iter().cloned());
            out
        }
        None => cmd.to_vec(),
    }
}

// ============================================================================
// D-Bus session bus — the SINGLE call point for the whole project
// ============================================================================
//
// Why it is here and not at the call site (rule #12): half of the modules
// used to call the `qdbus` binary directly, and most target systems have no
// such binary at all:
//   • Arch / CachyOS / SteamOS ....... qdbus6      (package qt6-tools)
//   • Debian / Ubuntu / Kubuntu ...... qdbus-qt6   (package qdbus-qt6)
//   • Fedora / openSUSE .............. qdbus-qt6   (package qt6-qttools)
//   • the Qt5 era (and NixOS) ........ qdbus / qdbus-qt5
// and on top of that Kubuntu puts the binary in /usr/lib/qt6/bin/, where
// PATH does not look at all — iterating names does not cure that.
//
// So the MAIN path is `dbus-send`: it comes from the `dbus` package, i.e.
// lies on any machine that has a session bus at all, and has the same name
// everywhere. `qdbus` stays a fallback.
//
// Their output formats DIFFER, so the answer is brought to one form («as
// qdbus prints it»): a bare value without type and quotes.

/// Order of preference: (canonical tool name, kind for the caller).
pub const DBUS_TOOL_PREFERENCE: &[(&str, &str)] = &[("dbus-send", "dbus-send"), ("qdbus6", "qdbus")];

/// Message when there is nothing to reach the bus with.
pub fn msg_no_dbus_tool(qdbus_names: &[&str]) -> String {
    crate::i18n::trf(
        // TRANSLATORS: %1 lists the qdbus program names that were looked for.
        "Neither dbus-send nor qdbus (%1) is installed, so Rexon can't talk to the \
         D-Bus session bus. Install the dbus package (dbus-bin on Debian/Ubuntu, \
         dbus-tools on Fedora).",
        &[&qdbus_names.join(", ")],
    )
}

/// A D-Bus method argument in the form `dbus-send` accepts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbusArg {
    /// A boolean (`boolean:true`).
    Bool(bool),
    /// An integer (`int32:…`).
    Int(i64),
    /// Everything else — as a string, the way Python prints it.
    Str(String),
}

impl DbusArg {
    /// The argument notation for `dbus-send` (`string:…`).
    pub fn to_dbus_send(&self) -> String {
        match self {
            // The order matters: in Python bool is a subclass of int, and it is
            // checked first, otherwise True would end up as int32:1.
            DbusArg::Bool(v) => format!("boolean:{}", if *v { "true" } else { "false" }),
            DbusArg::Int(v) => format!("int32:{v}"),
            DbusArg::Str(v) => format!("string:{v}"),
        }
    }

    /// The argument notation for `qdbus` — just the value as a string.
    pub fn to_qdbus(&self) -> String {
        match self {
            DbusArg::Bool(v) => (if *v { "True" } else { "False" }).to_string(),
            DbusArg::Int(v) => v.to_string(),
            DbusArg::Str(v) => v.clone(),
        }
    }
}

/// A `dbus-send` answer → a bare value, the way qdbus would print it.
///
/// The first line is service information («method return time=…»), then come
/// values with their types; compound ones (`array [`, `dict entry(`) are
/// skipped — scalars are needed. Quotes around a string value are removed.
pub fn parse_dbus_send_reply(out: &str) -> String {
    let mut values: Vec<&str> = Vec::new();
    for line in out.lines().skip(1) {
        let Some(val) = dbus_scalar_value(line) else {
            continue;
        };
        let val = if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
            &val[1..val.len() - 1]
        } else {
            val
        };
        values.push(val);
    }
    values.join("\n")
}

/// Scalar types in `dbus-send` output. `object path` is two words.
const DBUS_SCALAR_TYPES: &[&str] = &[
    "string",
    "object path",
    "signature",
    "boolean",
    "byte",
    "double",
    "int16",
    "int32",
    "int64",
    "uint16",
    "uint32",
    "uint64",
];

/// The value from a line like «   variant   string "x"  », if it is a scalar.
///
/// Port of the original's regular expression; a separate function so the core
/// does not get a regex engine dependency for the sake of one pattern.
fn dbus_scalar_value(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    // `variant` is an optional wrapper, a space is mandatory after it.
    let rest = match rest.strip_prefix("variant") {
        Some(r) if starts_with_space(r) => r.trim_start(),
        _ => rest,
    };
    for ty in DBUS_SCALAR_TYPES {
        // `object path` in the output is separated by any number of spaces.
        let after = if let Some((first, second)) = ty.split_once(' ') {
            let Some(r) = rest.strip_prefix(first) else {
                continue;
            };
            if !starts_with_space(r) {
                continue;
            }
            match r.trim_start().strip_prefix(second) {
                Some(r) => r,
                None => continue,
            }
        } else {
            match rest.strip_prefix(*ty) {
                Some(r) => r,
                None => continue,
            }
        };
        // The type must be followed by a space, otherwise it is another word («stringy»).
        if !starts_with_space(after) {
            continue;
        }
        return Some(after.trim());
    }
    None
}

/// Whether the line starts with a whitespace character.
fn starts_with_space(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_whitespace)
}

/// argv for calling a method on the session bus.
///
/// `kind` — which binary (`dbus-send` or `qdbus`, see
/// [`DBUS_TOOL_PREFERENCE`]): they have different argument order and
/// notation, and that is exactly why argv assembly lives in one place.
pub fn dbus_call_argv(
    kind: &str,
    exe: &str,
    dest: &str,
    path: &str,
    method: &str,
    args: &[DbusArg],
) -> Vec<String> {
    let mut cmd: Vec<String> = if kind == "dbus-send" {
        vec![
            exe.to_string(),
            "--session".into(),
            "--print-reply".into(),
            format!("--dest={dest}"),
            "--type=method_call".into(),
            path.to_string(),
            method.to_string(),
        ]
    } else {
        vec![
            exe.to_string(),
            dest.to_string(),
            path.to_string(),
            method.to_string(),
        ]
    };
    if kind == "dbus-send" {
        cmd.extend(args.iter().map(DbusArg::to_dbus_send));
    } else {
        cmd.extend(args.iter().map(DbusArg::to_qdbus));
    }
    cmd
}

/// Service, object and method for live KWin to reload its config.
pub const KWIN_RECONFIGURE: (&str, &str, &str) = ("org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure");

/// Service, object and method for running JS in plasmashell.
pub const PLASMA_EVALUATE_SCRIPT: (&str, &str, &str) = (
    "org.kde.plasmashell",
    "/PlasmaShell",
    "org.kde.PlasmaShell.evaluateScript",
);

/// Application name in notifications.
pub const NOTIFY_APP_NAME: &str = "Rexon";

/// argv for showing a notification directly over the bus.
///
/// We go to `org.freedesktop.Notifications`, not through the `notify-send`
/// binary: its package (libnotify / libnotify-bin) is not installed by
/// default on Debian 13, openSUSE or bare Arch — notifications simply did
/// not appear there, and silently. The notification service itself exists in
/// any graphical environment.
///
/// The `Notify` signature is `susssasa{sv}i`: the empty actions array and
/// hints dictionary are written as `array:string:` and `dict:string:string:`.
pub fn notify_dbus_argv(
    exe: &str,
    summary: &str,
    body: &str,
    icon: &str,
    timeout_ms: i64,
) -> Vec<String> {
    vec![
        exe.to_string(),
        "--session".into(),
        "--print-reply".into(),
        "--dest=org.freedesktop.Notifications".into(),
        "--type=method_call".into(),
        "/org/freedesktop/Notifications".into(),
        "org.freedesktop.Notifications.Notify".into(),
        format!("string:{NOTIFY_APP_NAME}"),
        "uint32:0".into(),
        format!("string:{icon}"),
        format!("string:{summary}"),
        format!("string:{body}"),
        "array:string:".into(),
        "dict:string:string:".into(),
        format!("int32:{timeout_ms}"),
    ]
}

/// argv of the fallback path via the `notify-send` binary.
pub fn notify_send_argv(exe: &str, summary: &str, body: &str, icon: &str) -> Vec<String> {
    vec![
        exe.to_string(),
        format!("--app-name={NOTIFY_APP_NAME}"),
        "--icon".into(),
        icon.to_string(),
        summary.to_string(),
        body.to_string(),
    ]
}

/// Show a desktop notification (the notification service over the bus,
/// otherwise `notify-send`). `false` — nothing to show it with. For messages
/// without a window: e.g. «Rexon did not restart», when its own window is
/// already gone.
pub fn notify_live(summary: &str, body: &str) -> bool {
    let path_env = std::env::var("PATH").ok();
    let argv = if let Some(exe) = which("dbus-send", path_env.as_deref()) {
        notify_dbus_argv(&exe, summary, body, "rexon", 0)
    } else if let Some(exe) = which("notify-send", path_env.as_deref()) {
        notify_send_argv(&exe, summary, body, "rexon")
    } else {
        return false;
    };
    let spec = RunSpec { argv, output: RunOutput::Capture, timeout_secs: Some(10), ..RunSpec::default() };
    run(&spec, path_env.as_deref()).outcome == RunOutcome::Ok
}

/// argv of a `notify-send` that waits for an answer: buttons `(key, label)`
/// (`--action`), the pressed one arrives in stdout as its key
/// ([`notify_answer`]). An important one does not fade by itself
/// (`--urgency=critical`).
pub fn notify_ask_argv(exe: &str, summary: &str, body: &str, actions: &[(String, String)]) -> Vec<String> {
    let mut argv = vec![
        exe.to_string(),
        format!("--app-name={NOTIFY_APP_NAME}"),
        "--icon=rexon".to_string(),
        "--urgency=critical".to_string(),
        "--wait".to_string(),
    ];
    for (key, label) in actions {
        argv.push(format!("--action={key}={label}"));
    }
    argv.push(summary.to_string());
    argv.push(body.to_string());
    argv
}

/// The pressed button from `notify-send --wait` output: the last non-empty
/// line, if it is the key of one of the `actions`. `None` — closed without an
/// answer.
pub fn notify_answer(stdout: &str, actions: &[(String, String)]) -> Option<String> {
    let last = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    actions.iter().find(|(k, _)| k == last).map(|(k, _)| k.clone())
}

/// Whether `notify-send` does not know buttons: libnotify before 0.7.9
/// (Ubuntu 20.04 and older) answers `--action`/`--wait` with «Unknown option».
pub fn notify_send_lacks_actions(stderr: &str) -> bool {
    let e = stderr.to_lowercase();
    e.contains("unknown option") && (e.contains("action") || e.contains("wait"))
}

/// A notification that waits for the person's action: with `actions` buttons
/// if `notify-send` can do them — and the answer (the pressed key) after it
/// is pressed. No buttons (no `notify-send`, or it is old) — the same
/// notification the usual way ([`notify_live`]) with the fallback text
/// `no_buttons`: what to do oneself. `wait_secs` — how long to wait for the
/// answer. Do not call from the UI thread: it waits.
pub fn notify_ask_live(
    summary: &str,
    body: &str,
    actions: &[(String, String)],
    no_buttons: &str,
    wait_secs: u64,
) -> Option<String> {
    let path_env = std::env::var("PATH").ok();
    if let Some(exe) = which("notify-send", path_env.as_deref()) {
        let spec = RunSpec {
            argv: notify_ask_argv(&exe, summary, body, actions),
            output: RunOutput::Capture,
            timeout_secs: Some(wait_secs),
            ..RunSpec::default()
        };
        let report = run(&spec, path_env.as_deref());
        if !notify_send_lacks_actions(&String::from_utf8_lossy(&report.stderr)) {
            return notify_answer(&String::from_utf8_lossy(&report.stdout), actions);
        }
    }
    let text = if no_buttons.is_empty() { body.to_string() } else { format!("{body}\n\n{no_buttons}") };
    notify_live(summary, &text);
    None
}

// ============================================================================
// External command executor — the ONLY place where Rexon spawns a process
// ============================================================================
//
// WHY IT IS HERE. Launch decisions already lived in the core: which timeout
// for which command (`run_duration`), under what name the binary lies on this
// machine (`resolved_argv`), which command to build at all (`system_plan`
// plans). But the launch itself stayed in Python — `subprocess.run` in
// `rexon/utils.py`. Because of this seam the Rust part of the product could
// not carry a single action to the end: the plan was built in the core and
// hit the boundary beyond which live Python was needed.
//
// Now the launch is here, and it is one for the whole product. Python keeps
// no copy of its own: `rexon/utils.py::safe_run` calls this function through
// the bridge and only brings the answer into the form of
// `subprocess.CompletedProcess`.
//
// ⚠️ WHAT EXACTLY MOVED, besides the `spawn` itself. Three things, each the
// price of a past defect:
//   • the timeout by the command's CLASS, not one for everything (`pacman -Q`
//     and a build of nine AUR packages were measured by the same thirty
//     seconds);
//   • `LANG=C`/`LC_ALL=C` for commands whose output is PARSED: on a Russian
//     locale package managers print translated fields, and the core's regular
//     expressions find nothing. Previously Python itself added this pair in
//     place, looking at the `lang_c` flag in the plan — that is, the core's
//     decision was executed by two different lines in two files;
//   • `stdin` to /dev/null. None of our commands reads the keyboard, but the
//     child inherited stdin, and a question like «Enter a selection
//     (default=all):» would hang on a live but silent input until the
//     timeout ran out — and the build timeout is four hours.

/// Launch outcome — so that «could not» and «did not wait long enough» cannot
/// be confused.
///
/// The former `safe_run` returned `None` for both cases, and the caller
/// honestly wrote «was not installed» to the person where time simply ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The command ran, exit code 0.
    Ok,
    /// The command ran and returned a non-zero code.
    Failed,
    /// Did not fit into the timeout — this is NOT the same as «could not».
    Timeout,
    /// Did not start at all: no binary, no permissions, the output file did not open.
    Error,
}

impl RunOutcome {
    /// Short outcome name — for the log and for the bridge to Python.
    pub fn as_str(self) -> &'static str {
        match self {
            RunOutcome::Ok => "ok",
            RunOutcome::Failed => "failed",
            RunOutcome::Timeout => "timeout",
            RunOutcome::Error => "error",
        }
    }
}

/// Where to put the command's output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RunOutput {
    /// Inherit the parent's output (`subprocess.run` without `capture_output`).
    #[default]
    Inherit,
    /// Capture into memory as TEXT: utf-8 with replacement of invalid bytes and
    /// universal newlines — exactly what `text=True` does.
    Capture,
    /// Capture into memory as BYTES: that is how `run_tool` works (converters
    /// return not only text), and newline translation there would corrupt data.
    CaptureBytes,
    /// Write to a file. That is how a long installation goes: the output goes to
    /// a file, and the live progress is read from it separately without
    /// disturbing the process.
    File(String),
}

/// The customer's language as a string, the way the environment sets it:
/// `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG` — in the order the locale
/// system itself reads them. `LANGUAGE` is a colon-separated list, the first
/// is taken.
///
/// `C`/`POSIX` means «no locale», and substituting `ru` there «because the
/// product is Russian» is not allowed: Russian text would show up on an
/// English machine. So an empty one answers `en`.
///
/// Lives here and not with the callers: the same loop is needed both for
/// translating plugin labels in the Qt layer and for human theme names in the
/// installed scanner (rule #12 — one engine, not a copy at each place).
pub fn env_locale() -> String {
    for name in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            let first = value.split(':').next().unwrap_or("").trim();
            if !first.is_empty() && first != "C" && first != "POSIX" {
                return first.to_string();
            }
        }
    }
    String::from("en")
}

/// Variables that mute the locale when the command's output is PARSED.
///
/// Both, not one: `LC_ALL` overrides the user's `LC_MESSAGES`, `LANG` covers
/// the case when `LC_ALL` is not set in the system at all.
pub const PARSED_OUTPUT_ENV: &[(&str, &str)] = &[("LANG", "C"), ("LC_ALL", "C")];

/// Variables that are REMOVED when the command's output is parsed.
///
/// The Rexon window sets `LANGUAGE` itself — the Rexon interface language
/// (`cli::qt_ui_language_env`), and every child process inherits it. GNU
/// gettext does not read it under `LC_ALL=C`, but Qt and KDE take the
/// interface language from it bypassing `LC_ALL` — parsed output does not
/// need it at all.
pub const PARSED_OUTPUT_ENV_REMOVE: &[&str] = &["LANGUAGE"];

/// Environment for a command whose output is PARSED by code: the C locale
/// ([`PARSED_OUTPUT_ENV`]) and no interface language
/// ([`PARSED_OUTPUT_ENV_REMOVE`]).
///
/// One point for the whole product (rule #12): [`run`] calls it with
/// `lang_c`, and so do the places that build a `Command` themselves. ⛔ Not
/// for commands that LAUNCH a program for the person (a desktop shell, a
/// player, a settings window): it would inherit the C locale and speak
/// English. And not for commands whose output is only SHOWN — it stays in the
/// person's language.
pub fn parsed_output_locale(cmd: &mut std::process::Command) -> &mut std::process::Command {
    for name in PARSED_OUTPUT_ENV_REMOVE {
        cmd.env_remove(name);
    }
    for (k, v) in PARSED_OUTPUT_ENV {
        cmd.env(k, v);
    }
    cmd
}

/// An order to launch an external command.
#[derive(Debug, Clone, Default)]
pub struct RunSpec {
    /// The command as is, before binary synonym substitution.
    pub argv: Vec<String>,
    /// Where to put the output.
    pub output: RunOutput,
    /// Timeout, seconds. `None` — take it by the command's class.
    pub timeout_secs: Option<u64>,
    /// The command's output will be parsed → mute the locale (`PARSED_OUTPUT_ENV`).
    pub lang_c: bool,
    /// ADDITIONS to the environment (not a replacement): «name, value» pairs.
    pub env: Vec<(String, String)>,
    /// stderr into the same stream as stdout (`stderr=subprocess.STDOUT`).
    pub stderr_to_stdout: bool,
}

/// What exactly will be launched — WITHOUT launching.
///
/// Separate from [`run`] because half of our commands cannot be executed in a
/// check: behind `pkexec` there is a desktop install and a password prompt.
/// The yardstick for such code is the textual parity of the built command,
/// and here it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// argv after binary synonym substitution (`kwriteconfig6` → `kwriteconfig5`).
    pub argv: Vec<String>,
    /// Final timeout, seconds.
    pub timeout_secs: u64,
    /// The run-time class the timeout was taken from.
    pub duration: RunDuration,
    /// Environment additions in the order applied: first muting the locale, then
    /// what the caller asked for.
    pub env: Vec<(String, String)>,
    /// Text mode with replacement of invalid bytes.
    pub text_mode: bool,
    /// stdin is closed to /dev/null (always: our commands have no keyboard).
    pub stdin_null: bool,
    /// The file stdout goes to, if set.
    pub stdout_file: Option<String>,
    /// stderr is merged into stdout.
    pub stderr_to_stdout: bool,
}

/// Lay out the order into a launch plan. Launches and changes nothing.
pub fn run_plan(spec: &RunSpec, path_env: Option<&str>) -> RunPlan {
    let argv = resolved_argv(&spec.argv, path_env);
    let duration = run_duration(&argv);
    let mut env: Vec<(String, String)> = Vec::new();
    if spec.lang_c {
        env.extend(
            PARSED_OUTPUT_ENV
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
        );
    }
    // AUR recipe build keys (`aur_install::BUILD_ENV`) — for any place that
    // calls an AUR helper through the shared launch (rule #12).
    env.extend(crate::aur_install::build_env(&spec.argv));
    env.extend(spec.env.iter().cloned());
    RunPlan {
        timeout_secs: spec.timeout_secs.unwrap_or_else(|| duration.timeout_secs()),
        duration,
        env,
        text_mode: matches!(spec.output, RunOutput::Capture),
        stdin_null: true,
        stdout_file: match &spec.output {
            RunOutput::File(p) => Some(p.clone()),
            _ => None,
        },
        stderr_to_stdout: spec.stderr_to_stdout,
        argv,
    }
}

/// Launch result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    /// What happened to the command.
    pub outcome: RunOutcome,
    /// Exit code; `None` — the process did not get to a code (timeout, did not start).
    pub returncode: Option<i32>,
    /// Collected stdout (empty if the output was not captured into memory).
    pub stdout: Vec<u8>,
    /// Collected stderr.
    pub stderr: Vec<u8>,
    /// The timeout it ran with — so that the message to the person names the number.
    pub timeout_secs: u64,
    /// Launch error text (only with [`RunOutcome::Error`]).
    pub error: String,
}

impl Default for RunOutcome {
    fn default() -> Self {
        RunOutcome::Error
    }
}

/// Output bytes → a string the way Python would return them in text mode.
///
/// `errors="replace"` is the same as `from_utf8_lossy` (both put U+FFFD).
/// Universal newlines (`\r\n` and a lone `\r` → `\n`): `text=True` does it,
/// and without it the lines of the live build progress would differ.
pub fn decode_text(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    if !s.contains('\r') {
        return s.into_owned();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Launch an external command. The ONLY process launch point in the product.
///
/// The timeout is by the command's class if the caller did not set their own;
/// the locale is muted when the output will be parsed; stdin is always
/// closed. No branch panics: «did not start» is [`RunOutcome::Error`] with
/// the cause text, not an exception.
pub fn run(spec: &RunSpec, path_env: Option<&str>) -> RunReport {
    // An AUR helper command with a package whose recipe is known to be broken:
    // Rexon builds that package itself (`aur_install::RECIPE_FIXES`) — for any
    // place that calls an AUR helper (rule #12).
    if let Some(report) = crate::aur_install::run_with_recipe_fixes(spec, path_env) {
        return report;
    }
    run_direct(spec, path_env)
}

/// [`run`] without the AUR recipe fix hook: the hook itself launches its
/// commands through here.
pub fn run_direct(spec: &RunSpec, path_env: Option<&str>) -> RunReport {
    let plan = run_plan(spec, path_env);
    let mut report = RunReport {
        timeout_secs: plan.timeout_secs,
        ..RunReport::default()
    };
    let Some((bin, rest)) = plan.argv.split_first() else {
        report.error = crate::i18n::tr("empty command");
        return report;
    };

    // Log: a command with elevated privileges — always (what is launched and
    // how it ended), the rest — only in the verbose log (`REXON_LOG=debug`).
    let privileged = crate::applog::is_privileged(&plan.argv);
    let log_level = if privileged { crate::applog::Level::Info } else { crate::applog::Level::Debug };
    let logged = crate::applog::enabled(log_level);
    if logged {
        crate::applog::write(
            log_level,
            if privileged { "privileged" } else { "run" },
            &format!("запуск: {}", crate::applog::redact_argv(&plan.argv)),
        );
    }
    let run_started = std::time::Instant::now();
    let report = run_inner(spec, &plan, bin, rest, report);
    if logged {
        crate::applog::write(
            log_level,
            if privileged { "privileged" } else { "run" },
            &format!(
                "{bin}: {:?}, код {} за {:.1} с{}",
                report.outcome,
                report.returncode.map_or_else(|| "—".to_string(), |c| c.to_string()),
                run_started.elapsed().as_secs_f64(),
                if report.error.is_empty() { String::new() } else { format!(" — {}", report.error) }
            ),
        );
    }
    report
}

fn run_inner(spec: &RunSpec, plan: &RunPlan, bin: &String, rest: &[String], mut report: RunReport) -> RunReport {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let capture = matches!(spec.output, RunOutput::Capture | RunOutput::CaptureBytes);
    let mut cmd = Command::new(bin);
    cmd.args(rest).stdin(Stdio::null());
    if spec.lang_c {
        // `plan.env` already carries the C locale; here — also removing `LANGUAGE`.
        parsed_output_locale(&mut cmd);
    }
    for (k, v) in &plan.env {
        cmd.env(k, v);
    }
    match &spec.output {
        RunOutput::Inherit => {
            cmd.stdout(Stdio::inherit());
        }
        RunOutput::Capture | RunOutput::CaptureBytes => {
            cmd.stdout(Stdio::piped());
        }
        // ⚠️ The file is opened ONCE and in append mode, and stderr gets a
        // copy of the same descriptor. Previously stderr was opened
        // separately: two opens have different write positions, and stdout
        // overwrote stderr lines (exactly the error messages vanished from
        // the AUR helper's log — sudo/pkexec write to stderr).
        RunOutput::File(path) => match fs::File::create(path)
            .and_then(|_| fs::OpenOptions::new().append(true).open(path))
        {
            Ok(f) => {
                if spec.stderr_to_stdout {
                    match f.try_clone() {
                        Ok(dup) => {
                            cmd.stderr(Stdio::from(dup));
                        }
                        Err(e) => {
                            report.error = msg_run_failed(bin, &e.to_string());
                            return report;
                        }
                    }
                }
                cmd.stdout(Stdio::from(f));
            }
            Err(e) => {
                report.error = msg_run_failed(bin, &e.to_string());
                return report;
            }
        },
    }
    if spec.stderr_to_stdout {
        // The same sink as stdout: in Python this is `stderr=STDOUT`.
        match &spec.output {
            // Already handed over as a copy of the stdout descriptor (see above).
            RunOutput::File(_) => {}
            RunOutput::Capture | RunOutput::CaptureBytes => {
                cmd.stderr(Stdio::piped());
            }
            RunOutput::Inherit => {
                cmd.stderr(Stdio::inherit());
            }
        }
    } else if capture {
        cmd.stderr(Stdio::piped());
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            report.error = msg_run_failed(bin, &e.to_string());
            return report;
        }
    };

    // Readers in their own threads: a full pipe would hang both the child and us.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let t_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let code = wait_secs(&mut child, plan.timeout_secs);
    report.stdout = t_out.join().unwrap_or_default();
    report.stderr = t_err.join().unwrap_or_default();
    // stderr merged into stdout via a pipe is appended to stdout — the way a
    // shared descriptor does it with `stderr=STDOUT`.
    if spec.stderr_to_stdout && capture {
        let tail = std::mem::take(&mut report.stderr);
        report.stdout.extend_from_slice(&tail);
    }
    match code {
        Some(rc) => {
            report.returncode = Some(rc);
            report.outcome = if rc == 0 {
                RunOutcome::Ok
            } else {
                RunOutcome::Failed
            };
        }
        None => {
            report.outcome = RunOutcome::Timeout;
            // What was collected before the timeout is NOT returned:
            // `subprocess.run` raises an exception in this case, and the caller
            // never saw the output.
            report.stdout.clear();
            report.stderr.clear();
        }
    }
    report
}

/// Wait for the process no longer than `timeout` seconds; `None` — did not
/// wait long enough.
///
/// On timeout the process is killed and reaped so as not to leave an orphan —
/// exactly like `subprocess.run(timeout=…)`.
fn wait_secs(child: &mut std::process::Child, timeout: u64) -> Option<i32> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(exit_code(st)),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// Exit code the way Python sees it: a process killed by a signal is the
/// negative signal number (`-15`), not an empty place.
fn exit_code(st: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    st.code().unwrap_or_else(|| -st.signal().unwrap_or(0))
}

// ============================================================================
// Trash (freedesktop.org Trash 1.0)
// ============================================================================
//
// WHY OUR OWN rather than `gio trash`. The trash is needed where Rexon
// removes something it did not install (a Conky widget from `~/.conky`): a
// mistake is possible, and the item must come back from the file manager's
// trash. `gio` is part of GLib and may be missing on a stripped-down machine
// (SteamOS, KDE without GTK programs), and its exit code does not say
// whether the file was moved or copied. The specification is simple, and
// here it is implemented fully where reliability matters:
//   * the file is MOVED (`rename(2)`), not copied: on the same partition it
//     is one atomic operation, the item cannot «get lost halfway»;
//   * on another partition — that partition's trash (`$topdir/.Trash/$uid`
//     if the administrator set it up properly, otherwise
//     `$topdir/.Trash-$uid`), also by moving. We will NEVER copy between
//     partitions and erase the original: if moving failed, the item stays in
//     place, and it is said why;
//   * the name in the trash is claimed by creating `info/<name>.trashinfo`
//     with `O_EXCL` BEFORE the move (as the specification requires): two
//     programs putting a same-named item at the same time will not overwrite
//     each other. If the move failed, the record is removed;
//   * a symlink goes to the trash itself, its target is not touched.

/// The «home» trash directory: `$XDG_DATA_HOME/Trash` (otherwise
/// `~/.local/share/Trash`).
pub fn home_trash_dir(xdg_data_home: Option<&str>, home: &str) -> String {
    join_path(&data_home(xdg_data_home, home), "Trash")
}

/// Path for the `Path=` line of a trash record: bytes outside the URI-«safe»
/// ones (`A-Z a-z 0-9 - _ . ~` and `/`) become `%XX`, as the specification
/// requires (RFC 2396). A space is `%20`, Cyrillic — by UTF-8 bytes.
pub fn trash_escape_path(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Text of a `.trashinfo` record. `path` — an already escaped path
/// ([`trash_escape_path`]), `date` — local time `YYYY-MM-DDThh:mm:ss`.
pub fn trash_info_text(path: &str, date: &str) -> String {
    format!("[Trash Info]\nPath={path}\nDeletionDate={date}\n")
}

/// The `DeletionDate` stamp for «now» — local time, without a time zone (as
/// in the specification). The clock cannot be read — midnight of 1970: file
/// managers do not show a trash record without a date at all.
pub fn trash_deletion_date_now() -> String {
    crate::qt_app::Timestamp::now_local()
        .map(|t| format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second))
        .unwrap_or_else(|| "1970-01-01T00:00:00".to_string())
}

/// Where things were put when moved to the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trashed {
    /// The item in the trash (`<trash>/files/<name>`).
    pub file: PathBuf,
    /// Its record (`<trash>/info/<name>.trashinfo`).
    pub info: PathBuf,
}

/// Move `path` to the trash. `home_trash` — [`home_trash_dir`]; `date` —
/// [`trash_deletion_date_now`] (passed from outside for the sake of checks).
/// `Err` — the cause text; then nothing changed on disk.
pub fn move_to_trash(path: &Path, home_trash: &Path, date: &str) -> Result<Trashed, String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    let name = path.file_name().ok_or_else(|| format!("no file name: {}", path.display()))?;
    let parent = path.parent().ok_or_else(|| format!("no parent folder: {}", path.display()))?;
    fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    // The partition is determined by the directory holding the entry (for a link
    // to a directory — by the real directory), not by the widget link's target.
    let dev = fs::metadata(parent).map_err(|e| e.to_string())?.dev();
    let (trash, info_path) = match ensure_trash(home_trash) {
        Ok(()) if fs::metadata(home_trash).map(|m| m.dev()).ok() == Some(dev) => {
            (home_trash.to_path_buf(), trash_escape_path(path.as_os_str().as_bytes()))
        }
        _ => {
            let real_parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
            let top = mount_top(&real_parent, dev);
            let trash = topdir_trash(&top)?;
            let rel = real_parent.join(name);
            let rel = rel.strip_prefix(&top).unwrap_or(&rel);
            (trash, trash_escape_path(rel.as_os_str().as_bytes()))
        }
    };
    let text = trash_info_text(&info_path, date);
    let stem = name.to_string_lossy().into_owned();
    for n in 1..=1000u32 {
        let candidate = if n == 1 { stem.clone() } else { format!("{stem}.{n}") };
        let file = trash.join("files").join(&candidate);
        let info = trash.join("info").join(format!("{candidate}.trashinfo"));
        if fs::symlink_metadata(&file).is_ok() {
            continue;
        }
        let created = fs::OpenOptions::new().write(true).create_new(true).open(&info);
        let mut f = match created {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        use std::io::Write;
        if let Err(e) = f.write_all(text.as_bytes()).and_then(|()| f.sync_all()) {
            let _ = fs::remove_file(&info);
            return Err(e.to_string());
        }
        drop(f);
        return match fs::rename(path, &file) {
            Ok(()) => Ok(Trashed { file, info }),
            Err(e) => {
                let _ = fs::remove_file(&info);
                Err(e.to_string())
            }
        };
    }
    Err("no free name in the trash".to_string())
}

/// Create the trash's `files/` and `info/` (mode 0700 — the trash contents
/// are not visible to others).
fn ensure_trash(trash: &Path) -> std::io::Result<()> {
    for sub in ["files", "info"] {
        let dir = trash.join(sub);
        if !dir.is_dir() {
            fs::create_dir_all(&dir)?;
            let _ = fs::set_permissions(trash, fs::Permissions::from_mode(0o700));
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Mount point of the partition `dev` on which `dir` lies (a real path).
fn mount_top(dir: &Path, dev: u64) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let mut top = dir.to_path_buf();
    while let Some(up) = top.parent() {
        if fs::metadata(up).map(|m| m.dev()).ok() != Some(dev) {
            break;
        }
        top = up.to_path_buf();
    }
    top
}

/// Trash of the partition rooted at `top`: `$top/.Trash/$uid` if `.Trash` is a
/// real directory (not a link) with the sticky bit, otherwise
/// `$top/.Trash-$uid`. Someone else's or a non-directory under this name —
/// refusal.
fn topdir_trash(top: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid takes no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    let shared = top.join(".Trash");
    if let Ok(m) = fs::symlink_metadata(&shared) {
        if m.is_dir() && m.mode() & 0o1000 != 0 {
            let mine = shared.join(uid.to_string());
            if ensure_trash(&mine).is_ok() && owned_dir(&mine, uid) {
                return Ok(mine);
            }
        }
    }
    let own = top.join(format!(".Trash-{uid}"));
    if fs::symlink_metadata(&own).is_err() {
        fs::create_dir(&own).map_err(|e| format!("{}: {e}", own.display()))?;
        fs::set_permissions(&own, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    if !owned_dir(&own, uid) {
        return Err(format!("{} is not a trash folder of this user", own.display()));
    }
    ensure_trash(&own).map_err(|e| format!("{}: {e}", own.display()))?;
    Ok(own)
}

/// A directory (not a link) and owned by `uid`.
fn owned_dir(p: &Path, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && m.uid() == uid)
}

#[cfg(test)]
mod notify_ask_tests {
    use super::*;

    /// Notification with buttons: argv `notify-send --wait --action=…`, the
    /// answer is the pressed key; an old `notify-send` without buttons is
    /// recognized by its answer.
    #[test]
    fn notify_with_buttons_and_answer() {
        let actions = vec![("open".to_string(), "Открыть".to_string())];
        let argv = notify_ask_argv("/usr/bin/notify-send", "T", "B", &actions);
        assert_eq!(argv[0], "/usr/bin/notify-send");
        assert!(argv.contains(&"--wait".to_string()) && argv.contains(&"--action=open=Открыть".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["T", "B"]);
        assert_eq!(notify_answer("\nopen\n", &actions).as_deref(), Some("open"));
        assert_eq!(notify_answer("", &actions), None, "закрыли без ответа");
        assert_eq!(notify_answer("other\n", &actions), None);
        assert!(notify_send_lacks_actions("Unknown option --action=open=Открыть"));
        assert!(notify_send_lacks_actions("notify-send: Unknown option --wait"));
        assert!(!notify_send_lacks_actions(""));
    }
}

#[cfg(test)]
mod trash_tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rexon-trash-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn escape_keeps_slashes_and_encodes_space_and_cyrillic() {
        assert_eq!(trash_escape_path(b"/home/u/.conky/Gotham"), "/home/u/.conky/Gotham");
        assert_eq!(trash_escape_path("/home/u/Мои виджеты/a%b".as_bytes()), "/home/u/%D0%9C%D0%BE%D0%B8%20%D0%B2%D0%B8%D0%B4%D0%B6%D0%B5%D1%82%D1%8B/a%25b");
        assert_eq!(trash_info_text("/a", "2026-10-19T12:00:05"), "[Trash Info]\nPath=/a\nDeletionDate=2026-10-19T12:00:05\n");
        let now = trash_deletion_date_now();
        assert_eq!(now.len(), 19, "{now}");
        assert_eq!(&now[10..11], "T");
    }

    /// The folder is moved whole, the trash record is correct; a second one with
    /// the same name goes under a free name, the first is not overwritten.
    #[test]
    fn folder_moves_with_info_and_names_do_not_clash() {
        let base = sandbox("dir");
        let trash = base.join("data/Trash");
        let w = base.join("Мои виджеты/Gotham");
        fs::create_dir_all(&w).unwrap();
        fs::write(w.join("Gotham"), "TEXT\n").unwrap();
        let t = move_to_trash(&w, &trash, "2026-10-19T01:02:03").unwrap();
        assert!(!w.exists());
        assert_eq!(t.file, trash.join("files/Gotham"));
        assert_eq!(fs::read_to_string(t.file.join("Gotham")).unwrap(), "TEXT\n");
        let want = format!(
            "[Trash Info]\nPath={}\nDeletionDate=2026-10-19T01:02:03\n",
            trash_escape_path(w.as_os_str().as_encoded_bytes())
        );
        assert_eq!(fs::read_to_string(&t.info).unwrap(), want);
        assert!(want.contains("%D0%9C%D0%BE%D0%B8%20"), "{want}");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(trash.join("info")).unwrap().permissions().mode() & 0o777, 0o700);

        fs::create_dir_all(&w).unwrap();
        let t2 = move_to_trash(&w, &trash, "2026-10-19T01:02:04").unwrap();
        assert_eq!(t2.file, trash.join("files/Gotham.2"));
        assert_eq!(t2.info, trash.join("info/Gotham.2.trashinfo"));
        assert!(t.file.join("Gotham").exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// The link goes to the trash itself; the target stays in place untouched.
    #[test]
    fn symlink_goes_to_trash_not_its_target() {
        let base = sandbox("link");
        let trash = base.join("Trash");
        let target = base.join("real/Gotham");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("conf"), "TEXT\n").unwrap();
        let link = base.join(".conky/Gotham");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let t = move_to_trash(&link, &trash, "2026-10-19T01:02:03").unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(fs::symlink_metadata(&t.file).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(target.join("conf")).unwrap(), "TEXT\n");
        let _ = fs::remove_dir_all(&base);
    }

    /// An item on another partition (`/dev/shm` is a separate tmpfs) goes to THAT
    /// partition's trash (`$topdir/.Trash-$uid`) by moving, the path in the
    /// record is relative to the partition root. No separate partition, or its
    /// trash already exists — the check is skipped (we do not touch someone
    /// else's trash).
    #[test]
    fn other_partition_uses_its_own_trash_by_move() {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        let base = sandbox("xdev");
        let uid = unsafe { libc::getuid() };
        let topdir_trash = shm.join(format!(".Trash-{uid}"));
        let other_dev = fs::metadata(shm).map(|m| m.dev()).ok().filter(|d| Some(*d) != fs::metadata(&base).map(|m| m.dev()).ok());
        if other_dev.is_none() || fs::symlink_metadata(&topdir_trash).is_ok() {
            let _ = fs::remove_dir_all(&base);
            return;
        }
        let w = shm.join(format!("rexon-trash-xdev-{}", std::process::id()));
        fs::create_dir_all(&w).unwrap();
        fs::write(w.join("c"), "TEXT\n").unwrap();
        let res = move_to_trash(&w, &base.join("Trash"), "2026-10-19T00:00:00");
        let moved = res.as_ref().map(|t| t.file.join("c").is_file()).unwrap_or(false);
        let info = res.as_ref().ok().and_then(|t| fs::read_to_string(&t.info).ok()).unwrap_or_default();
        let home_trash_used = base.join("Trash/files").read_dir().map(|mut d| d.next().is_some()).unwrap_or(false);
        let _ = fs::remove_dir_all(&w);
        let _ = fs::remove_dir_all(&topdir_trash);
        let _ = fs::remove_dir_all(&base);
        let t = res.unwrap();
        assert!(t.file.starts_with(&topdir_trash), "{t:?}");
        assert!(moved && !home_trash_used);
        let name = w.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(info, format!("[Trash Info]\nPath={name}\nDeletionDate=2026-10-19T00:00:00\n"));
    }

    /// Nothing to move or the path is not absolute — refusal, no record remains.
    #[test]
    fn missing_or_relative_is_refused_without_leftovers() {
        let base = sandbox("missing");
        let trash = base.join("Trash");
        assert!(move_to_trash(&base.join("nope"), &trash, "d").is_err());
        assert!(move_to_trash(Path::new("rel/x"), &trash, "d").is_err());
        assert!(move_to_trash(&base.join("a/../b"), &trash, "d").is_err());
        let infos = fs::read_dir(trash.join("info")).map(|d| d.count()).unwrap_or(0);
        assert_eq!(infos, 0);
        let _ = fs::remove_dir_all(&base);
    }
}
