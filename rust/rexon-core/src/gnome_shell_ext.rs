//! GNOME extensions through the shell itself: the D-Bus interface
//! `org.gnome.Shell.Extensions` at `/org/gnome/Shell` (session bus).
//!
//! It is not Rexon but GNOME Shell that installs, removes, enables and disables:
//! installation (`InstallRemoteExtension`) shows the shell's confirmation
//! dialog, the shell itself fetches the build for its version from
//! extensions.gnome.org and starts it immediately — without logging out again,
//! on Wayland too. Rexon only asks and honestly relays the answer.
//!
//! The bus connection is behind the [`ShellExtIo`] trait: the live
//! implementation [`LiveShell`] (zbus, a synchronous call from a worker thread)
//! and a fake in tests. All the logic — parsing `a{sv}`, states, refusals and
//! their understandable wording — is in free functions on top of the trait and
//! is tested without a shell.
//!
//! When there is no shell (Rexon is running in another environment, and the
//! GNOME section was opened by choosing the environment), there is the fallback
//! path [`install_zip_offline`]: the downloaded release zip is checked like an
//! `.rxn` (paths inside the archive, executable content) and installed with
//! `gnome-extensions install --force` or by unpacking into the uuid directory;
//! it will start working on the next GNOME login.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::i18n::{tr, trf};

// ── Bus constants ────────────────────────────────────────────────────────

/// Name of the shell service on the session bus.
pub const BUS_NAME: &str = "org.gnome.Shell";
/// The object with the extensions interface.
pub const OBJECT_PATH: &str = "/org/gnome/Shell";
/// The extensions interface.
pub const INTERFACE: &str = "org.gnome.Shell.Extensions";

/// Timeout of an ordinary call: the shell answers immediately.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(25);
/// Install timeout: the answer arrives only after a click in the shell's
/// confirmation dialog (and the download), and a person may think for minutes.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The shell's settings schema.
pub const SHELL_SCHEMA: &str = "org.gnome.shell";
/// The "extension installation allowed" key (`false` — the shell will refuse).
pub const ALLOW_INSTALL_KEY: &str = "allow-extension-installation";
/// The "user extensions disabled" key (the global switch).
pub const DISABLE_USER_KEY: &str = "disable-user-extensions";

/// `InstallRemoteExtension` answer on success.
pub const INSTALL_SUCCESSFUL: &str = "successful";
/// `InstallRemoteExtension` answer when the person pressed "Cancel" — not an error.
pub const INSTALL_CANCELLED: &str = "cancelled";

/// The GNOME utility for the fallback paths.
pub const GNOME_EXTENSIONS_BIN: &str = "gnome-extensions";

/// Limit on the unpacked size of a release zip (protection against "bombs"):
/// real extensions are a few megabytes.
pub const ZIP_UNPACKED_LIMIT: u64 = 64 * 1024 * 1024;
/// Limit on the number of entries in a release zip.
pub const ZIP_ENTRY_LIMIT: usize = 5000;
/// How much of the start of a file the executable classifier looks at.
const HEAD_BYTES: u64 = 4096;

// ── Raw bus data ─────────────────────────────────────────────────────────

/// A value from the shell's `a{sv}` dictionary, reduced to simple kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Str(String),
    /// The shell returns numbers as `d` (double): `state`, `type`, `version`.
    Num(f64),
    Bool(bool),
    /// `as` or `av` of strings (`shell-version`, `sessionModes`).
    List(Vec<String>),
    /// A dictionary with string values (`donations`).
    Map(BTreeMap<String, String>),
    /// Everything else — Rexon does not need it.
    Other,
}

/// The information dictionary of one extension (`a{sv}`).
pub type RawInfo = BTreeMap<String, Field>;

/// A call failure on the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusFailure {
    /// The name `org.gnome.Shell` has no owner (the shell is not running or is
    /// restarting), or there is no session bus.
    NoShell,
    /// No answer came in time.
    Timeout,
    /// The shell returned a D-Bus error.
    Method { name: String, message: String },
    /// Something else (answer parsing, input/output).
    Other(String),
}

/// Result of an external command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CmdOutput {
    pub success: bool,
    /// stderr, then stdout — for the message to the person.
    pub text: String,
}

/// Connection to the shell and the system. Live — [`LiveShell`], in tests — a fake.
pub trait ShellExtIo {
    /// Whether the name [`BUS_NAME`] has an owner on the session bus.
    fn shell_running(&mut self) -> bool;
    fn list(&mut self) -> Result<BTreeMap<String, RawInfo>, BusFailure>;
    fn info(&mut self, uuid: &str) -> Result<RawInfo, BusFailure>;
    fn errors(&mut self, uuid: &str) -> Result<Vec<String>, BusFailure>;
    /// `InstallRemoteExtension`; waits no longer than `timeout`.
    fn install_remote(&mut self, uuid: &str, timeout: Duration) -> Result<String, BusFailure>;
    fn uninstall(&mut self, uuid: &str) -> Result<bool, BusFailure>;
    fn enable(&mut self, uuid: &str) -> Result<bool, BusFailure>;
    fn disable(&mut self, uuid: &str) -> Result<bool, BusFailure>;
    /// `OpenExtensionPrefs(uuid, parent_window, {modal})`.
    fn open_prefs(&mut self, uuid: &str, parent_window: &str, modal: bool) -> Result<(), BusFailure>;
    /// The obsolete `LaunchExtensionPrefs(uuid)`.
    fn launch_prefs(&mut self, uuid: &str) -> Result<(), BusFailure>;
    fn check_for_updates(&mut self) -> Result<(), BusFailure>;
    /// The `ShellVersion` property.
    fn shell_version(&mut self) -> Result<String, BusFailure>;
    /// The `UserExtensionsEnabled` property.
    fn user_extensions_enabled(&mut self) -> Result<bool, BusFailure>;
    fn set_user_extensions_enabled(&mut self, on: bool) -> Result<(), BusFailure>;
    /// A boolean key of the [`SHELL_SCHEMA`] schema; `None` — cannot be read.
    fn shell_setting_bool(&mut self, key: &str) -> Option<bool>;
    /// Whether a command is in PATH.
    fn which(&mut self, cmd: &str) -> bool;
    /// Run a command and wait for it.
    fn run(&mut self, argv: &[String]) -> Result<CmdOutput, String>;
}

// ── Parsed information ───────────────────────────────────────────────────

/// Extension state (the `state` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtState {
    Active,
    Inactive,
    Error,
    /// The current shell version is not declared.
    OutOfDate,
    Downloading,
    Initialized,
    Deactivating,
    Activating,
    Uninstalled,
    Unknown(i64),
}

impl ExtState {
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => ExtState::Active,
            2 => ExtState::Inactive,
            3 => ExtState::Error,
            4 => ExtState::OutOfDate,
            5 => ExtState::Downloading,
            6 => ExtState::Initialized,
            7 => ExtState::Deactivating,
            8 => ExtState::Activating,
            99 => ExtState::Uninstalled,
            n => ExtState::Unknown(n),
        }
    }

    pub fn code(self) -> i64 {
        match self {
            ExtState::Active => 1,
            ExtState::Inactive => 2,
            ExtState::Error => 3,
            ExtState::OutOfDate => 4,
            ExtState::Downloading => 5,
            ExtState::Initialized => 6,
            ExtState::Deactivating => 7,
            ExtState::Activating => 8,
            ExtState::Uninstalled => 99,
            ExtState::Unknown(n) => n,
        }
    }

    /// Key for the interface (QML, catalog strings).
    pub fn key(self) -> &'static str {
        match self {
            ExtState::Active => "active",
            ExtState::Inactive => "inactive",
            ExtState::Error => "error",
            ExtState::OutOfDate => "out-of-date",
            ExtState::Downloading => "downloading",
            ExtState::Initialized => "initialized",
            ExtState::Deactivating => "deactivating",
            ExtState::Activating => "activating",
            ExtState::Uninstalled => "uninstalled",
            ExtState::Unknown(_) => "unknown",
        }
    }

    /// An error or outdated — the tile needs a trouble icon.
    pub fn is_problem(self) -> bool {
        matches!(self, ExtState::Error | ExtState::OutOfDate)
    }
}

/// Information about an installed extension.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtInfo {
    pub uuid: String,
    pub name: String,
    pub description: String,
    pub state: ExtState,
    pub enabled: bool,
    /// The last error from `init()/enable()/disable()`; empty — none.
    pub error: String,
    pub has_prefs: bool,
    /// The shell has already downloaded an update; it will apply on the next login.
    pub has_update: bool,
    /// `false` — cannot be changed (administrator, session mode).
    pub can_change: bool,
    /// System-wide (`type = 1`): cannot be removed.
    pub is_system: bool,
    pub path: String,
    pub url: String,
    /// Release number (`version`, an integer on EGO); empty — not declared.
    pub version: String,
    /// The author's release name (`version-name`).
    pub version_name: String,
    pub shell_versions: Vec<String>,
    pub session_modes: Vec<String>,
    pub donations: BTreeMap<String, String>,
}

fn field_str(raw: &RawInfo, key: &str) -> String {
    match raw.get(key) {
        Some(Field::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn field_bool(raw: &RawInfo, key: &str) -> Option<bool> {
    match raw.get(key) {
        Some(Field::Bool(b)) => Some(*b),
        Some(Field::Num(n)) => Some(*n != 0.0),
        _ => None,
    }
}

fn field_num(raw: &RawInfo, key: &str) -> Option<f64> {
    match raw.get(key) {
        Some(Field::Num(n)) => Some(*n),
        Some(Field::Str(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn field_list(raw: &RawInfo, key: &str) -> Vec<String> {
    match raw.get(key) {
        Some(Field::List(l)) => l.clone(),
        Some(Field::Str(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A number from `d` without the `.0` tail (`6.0` → `6`).
pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

impl ExtInfo {
    /// Parsing the shell's dictionary. `None` — an empty dictionary (that is how the
    /// shell answers an unknown uuid).
    pub fn from_raw(raw: &RawInfo) -> Option<Self> {
        let uuid = field_str(raw, "uuid");
        if uuid.is_empty() {
            return None;
        }
        let state = ExtState::from_code(field_num(raw, "state").map(|n| n as i64).unwrap_or(0));
        let modes = match field_list(raw, "sessionModes") {
            l if !l.is_empty() => l,
            _ => field_list(raw, "session-modes"),
        };
        Some(ExtInfo {
            name: match field_str(raw, "name") {
                n if n.is_empty() => uuid.clone(),
                n => n,
            },
            description: field_str(raw, "description"),
            // Old shells do not provide the `enabled` field — then go by the state.
            enabled: field_bool(raw, "enabled").unwrap_or(state == ExtState::Active),
            state,
            error: field_str(raw, "error"),
            has_prefs: field_bool(raw, "hasPrefs").unwrap_or(false),
            has_update: field_bool(raw, "hasUpdate").unwrap_or(false),
            // No field (shell before 3.38) — then there is no prohibition either.
            can_change: field_bool(raw, "canChange").unwrap_or(true),
            is_system: field_num(raw, "type").map(|t| t as i64) == Some(1),
            path: field_str(raw, "path"),
            url: field_str(raw, "url"),
            version: field_num(raw, "version").map(format_number).unwrap_or_default(),
            version_name: field_str(raw, "version-name"),
            shell_versions: field_list(raw, "shell-version"),
            session_modes: modes,
            donations: match raw.get("donations") {
                Some(Field::Map(m)) => m.clone(),
                _ => BTreeMap::new(),
            },
            uuid,
        })
    }

    /// Version for display: the author's release name, otherwise the number.
    pub fn display_version(&self) -> &str {
        if self.version_name.is_empty() { &self.version } else { &self.version_name }
    }
}

// ── Refusals ─────────────────────────────────────────────────────────────

/// How the installation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The shell installed and started the extension — it works right away.
    Installed,
    /// The person pressed "Cancel" in the shell's dialog — not an error.
    Cancelled,
    /// Fallback path: the files are in place, it will work on the next GNOME login.
    InstalledNextLogin,
}

/// Refusal of an action — each has its own phrase for the person ([`ShellExtError::message`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellExtError {
    ShellNotRunning,
    /// `allow-extension-installation = false`.
    InstallForbidden,
    /// `disable-user-extensions = true` / `UserExtensionsEnabled = false`.
    UserExtensionsDisabled,
    /// `canChange = false`.
    Locked,
    /// A system-wide extension cannot be removed.
    SystemExtension,
    NotInstalled,
    NoPrefs,
    /// Does not look like an extension uuid.
    BadUuid(String),
    /// The shell did not answer in time.
    Timeout,
    /// The shell answered "it did not work" without a reason.
    Refused,
    /// A bus or shell error (the text as is).
    Bus(String),
    /// The release archive did not pass the check.
    BadArchive(String),
    /// The fallback installation failed (command output or a file-system error).
    Offline(String),
}

impl ShellExtError {
    pub fn message(&self) -> String {
        match self {
            ShellExtError::ShellNotRunning => {
                tr("GNOME Shell isn't running, so extensions can't be managed right now.")
            }
            ShellExtError::InstallForbidden => tr("Installing extensions is forbidden in the system settings."),
            ShellExtError::UserExtensionsDisabled => tr("User extensions are turned off."),
            ShellExtError::Locked => {
                tr("This extension can't be changed: it is locked by the administrator or the session.")
            }
            ShellExtError::SystemExtension => {
                tr("This extension is part of the system and can't be removed from Rexon.")
            }
            ShellExtError::NotInstalled => tr("This extension isn't installed."),
            ShellExtError::NoPrefs => tr("This extension has no settings."),
            ShellExtError::BadUuid(u) => trf("'%1' is not a GNOME extension id.", &[u]),
            ShellExtError::Timeout => tr(
                "GNOME Shell didn't answer in time. If its confirmation window is still open, \
                 it may be hidden behind other windows.",
            ),
            ShellExtError::Refused => tr("GNOME Shell refused to do this."),
            ShellExtError::Bus(m) => trf("GNOME Shell reported an error: %1", &[m]),
            ShellExtError::BadArchive(m) => trf("The extension archive was rejected: %1", &[m]),
            ShellExtError::Offline(m) => trf("The extension couldn't be installed: %1", &[m]),
        }
    }
}

impl std::fmt::Display for ShellExtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ShellExtError {}

/// Caption of the fallback path.
pub fn next_login_note() -> String {
    tr("The extension will start the next time you log in to GNOME.")
}

fn check_uuid(uuid: &str) -> Result<(), ShellExtError> {
    if crate::rxn_manifest::is_extension_uuid(uuid) {
        Ok(())
    } else {
        Err(ShellExtError::BadUuid(uuid.to_string()))
    }
}

/// Bus failure → refusal; the answer itself gives no reason, so we ask the settings.
fn failure(io: &mut dyn ShellExtIo, f: BusFailure, installing: bool) -> ShellExtError {
    match f {
        BusFailure::NoShell => ShellExtError::ShellNotRunning,
        BusFailure::Timeout => ShellExtError::Timeout,
        BusFailure::Method { message, .. } | BusFailure::Other(message) => {
            if installing && io.shell_setting_bool(ALLOW_INSTALL_KEY) == Some(false) {
                ShellExtError::InstallForbidden
            } else if io.shell_setting_bool(DISABLE_USER_KEY) == Some(true) {
                ShellExtError::UserExtensionsDisabled
            } else {
                ShellExtError::Bus(message)
            }
        }
    }
}

fn require_shell(io: &mut dyn ShellExtIo) -> Result<(), ShellExtError> {
    if io.shell_running() { Ok(()) } else { Err(ShellExtError::ShellNotRunning) }
}

// ── Actions ──────────────────────────────────────────────────────────────

/// Everything installed (empty dictionaries are skipped), by uuid.
pub fn list_extensions(io: &mut dyn ShellExtIo) -> Result<Vec<ExtInfo>, ShellExtError> {
    require_shell(io)?;
    let raw = io.list().map_err(|f| failure(io, f, false))?;
    Ok(raw.values().filter_map(ExtInfo::from_raw).collect())
}

/// Information about one; `None` — not installed.
pub fn extension_info(io: &mut dyn ShellExtIo, uuid: &str) -> Result<Option<ExtInfo>, ShellExtError> {
    require_shell(io)?;
    let raw = io.info(uuid).map_err(|f| failure(io, f, false))?;
    Ok(ExtInfo::from_raw(&raw))
}

/// The extension's error log (for the "Error" dialog).
pub fn extension_errors(io: &mut dyn ShellExtIo, uuid: &str) -> Result<Vec<String>, ShellExtError> {
    require_shell(io)?;
    io.errors(uuid).map_err(|f| failure(io, f, false))
}

fn installed(io: &mut dyn ShellExtIo, uuid: &str) -> Result<ExtInfo, ShellExtError> {
    check_uuid(uuid)?;
    extension_info(io, uuid)?.ok_or(ShellExtError::NotInstalled)
}

/// Install through the shell: its confirmation dialog, its download, starting
/// immediately. Called from a worker thread — waits up to [`INSTALL_TIMEOUT`].
pub fn install_remote(io: &mut dyn ShellExtIo, uuid: &str) -> Result<InstallOutcome, ShellExtError> {
    check_uuid(uuid)?;
    require_shell(io)?;
    if io.shell_setting_bool(ALLOW_INSTALL_KEY) == Some(false) {
        return Err(ShellExtError::InstallForbidden);
    }
    match io.install_remote(uuid, INSTALL_TIMEOUT) {
        Ok(r) if r == INSTALL_SUCCESSFUL => Ok(InstallOutcome::Installed),
        Ok(r) if r == INSTALL_CANCELLED => Ok(InstallOutcome::Cancelled),
        Ok(r) => Err(ShellExtError::Bus(r)),
        Err(f) => Err(failure(io, f, true)),
    }
}

/// Remove (user extensions only).
pub fn uninstall(io: &mut dyn ShellExtIo, uuid: &str) -> Result<(), ShellExtError> {
    let info = installed(io, uuid)?;
    if info.is_system {
        return Err(ShellExtError::SystemExtension);
    }
    match io.uninstall(uuid) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ShellExtError::Refused),
        Err(f) => Err(failure(io, f, false)),
    }
}

/// Enable or disable. The shell edits `enabled-extensions` itself.
pub fn set_enabled(io: &mut dyn ShellExtIo, uuid: &str, on: bool) -> Result<(), ShellExtError> {
    let info = installed(io, uuid)?;
    if !info.can_change {
        return Err(ShellExtError::Locked);
    }
    // The global switch prevents enabling a user extension: the shell would answer
    // "no" without a reason — we state the reason ourselves.
    if on && !info.is_system && io.user_extensions_enabled().ok() == Some(false) {
        return Err(ShellExtError::UserExtensionsDisabled);
    }
    let r = if on { io.enable(uuid) } else { io.disable(uuid) };
    match r {
        Ok(true) => Ok(()),
        Ok(false) if on && !info.is_system && io.shell_setting_bool(DISABLE_USER_KEY) == Some(true) => {
            Err(ShellExtError::UserExtensionsDisabled)
        }
        Ok(false) => Err(ShellExtError::Refused),
        Err(f) => Err(failure(io, f, false)),
    }
}

/// Open the extension's settings window: `OpenExtensionPrefs`, on refusal the
/// old `LaunchExtensionPrefs`, on refusal `gnome-extensions prefs`.
pub fn open_prefs(io: &mut dyn ShellExtIo, uuid: &str, parent_window: &str) -> Result<(), ShellExtError> {
    check_uuid(uuid)?;
    let mut last = None;
    if io.shell_running() {
        let info = installed(io, uuid)?;
        if !info.has_prefs {
            return Err(ShellExtError::NoPrefs);
        }
        match io.open_prefs(uuid, parent_window, false) {
            Ok(()) => return Ok(()),
            Err(f) => crate::log_warn!("gnome-ext", "OpenExtensionPrefs {uuid}: {f:?}"),
        }
        match io.launch_prefs(uuid) {
            Ok(()) => return Ok(()),
            Err(f) => {
                crate::log_warn!("gnome-ext", "LaunchExtensionPrefs {uuid}: {f:?}");
                last = Some(failure(io, f, false));
            }
        }
    }
    if !io.which(GNOME_EXTENSIONS_BIN) {
        return Err(last.unwrap_or(ShellExtError::ShellNotRunning));
    }
    let argv = [GNOME_EXTENSIONS_BIN.to_string(), "prefs".into(), uuid.to_string()];
    match io.run(&argv) {
        Ok(out) if out.success => Ok(()),
        Ok(out) => Err(ShellExtError::Bus(out.text.trim().to_string())),
        Err(e) => Err(ShellExtError::Bus(e)),
    }
}

/// Ask the shell to check for updates: it will check with the site itself,
/// download new releases and set `hasUpdate`.
pub fn check_for_updates(io: &mut dyn ShellExtIo) -> Result<(), ShellExtError> {
    require_shell(io)?;
    io.check_for_updates().map_err(|f| failure(io, f, false))
}

/// How many updates are downloaded and waiting for the login.
pub fn pending_updates(list: &[ExtInfo]) -> usize {
    list.iter().filter(|e| e.has_update).count()
}

/// Version of the running shell (`51.1`).
pub fn shell_version(io: &mut dyn ShellExtIo) -> Result<String, ShellExtError> {
    require_shell(io)?;
    io.shell_version().map_err(|f| failure(io, f, false))
}

/// GNOME Shell version of this machine for checking against the extensions
/// site: for a running shell — the `ShellVersion` property, without one — the
/// output of `gnome-shell --version` ("GNOME Shell 51.1"). `None` — there is no
/// GNOME Shell here.
pub fn detect_shell_version(io: &mut dyn ShellExtIo) -> Option<String> {
    if io.shell_running() {
        if let Ok(v) = io.shell_version() {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    if !io.which("gnome-shell") {
        return None;
    }
    let out = io.run(&["gnome-shell".to_string(), "--version".to_string()]).ok()?;
    if !out.success {
        return None;
    }
    version_from_output(&out.text)
}

/// Version number from the output of `gnome-shell --version`: the line "GNOME
/// Shell 51.1" (stderr warnings next to it are skipped).
pub fn version_from_output(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let v = l.trim().strip_prefix("GNOME Shell ")?.trim();
        v.chars().next().is_some_and(|c| c.is_ascii_digit()).then(|| v.to_string())
    })
}

/// The global "User extensions" switch.
pub fn user_extensions_enabled(io: &mut dyn ShellExtIo) -> Result<bool, ShellExtError> {
    require_shell(io)?;
    io.user_extensions_enabled().map_err(|f| failure(io, f, false))
}

pub fn set_user_extensions_enabled(io: &mut dyn ShellExtIo, on: bool) -> Result<(), ShellExtError> {
    require_shell(io)?;
    io.set_user_extensions_enabled(on).map_err(|f| failure(io, f, false))
}

// ── Fallback path: a release zip without the shell ───────────────────────

/// Result of checking a release zip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipCheck {
    pub uuid: String,
    /// Executable content (JavaScript is normal for an extension).
    pub executables: Vec<crate::rxn_manifest::ExecFinding>,
}

/// Check a release zip with the same rules as an `.rxn`: paths inside the
/// archive without `..`, `\`, NUL and absolute paths; `metadata.json` at the root
/// with the expected uuid; executable files (ELF) are forbidden — a GNOME
/// extension does not need them, and the site does not accept such; limits on
/// size and number of entries.
pub fn check_extension_zip(zip_path: &Path, expected_uuid: &str) -> Result<ZipCheck, ShellExtError> {
    use crate::rxn_manifest::{scan_executables, ArchiveFile, ExecClass};
    check_uuid(expected_uuid)?;
    let bad = |m: String| ShellExtError::BadArchive(m);
    let file = std::fs::File::open(zip_path).map_err(|e| bad(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    if archive.len() > ZIP_ENTRY_LIMIT {
        return Err(bad(tr("too many files")));
    }
    let mut heads: Vec<(String, Vec<u8>, bool)> = Vec::new();
    let mut metadata: Option<Vec<u8>> = None;
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| bad(e.to_string()))?;
        let name = entry.name().to_string();
        let safe = !name.is_empty()
            && !name.starts_with('/')
            && !name.contains('\\')
            && !name.contains('\0')
            && crate::appearance_installer::safe_extract_entry("/ext", &name);
        if !safe {
            return Err(bad(trf("path '%1' must stay inside the package", &[&name])));
        }
        total = total.saturating_add(entry.size());
        if total > ZIP_UNPACKED_LIMIT {
            return Err(bad(tr("the archive is too large")));
        }
        if entry.is_dir() {
            continue;
        }
        let executable = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
        let path = name.trim_start_matches("./").to_string();
        if path == "metadata.json" {
            let mut buf = Vec::new();
            (&mut entry).take(1024 * 1024).read_to_end(&mut buf).map_err(|e| bad(e.to_string()))?;
            heads.push((path, buf.iter().take(HEAD_BYTES as usize).copied().collect(), executable));
            metadata = Some(buf);
            continue;
        }
        let mut head = Vec::new();
        (&mut entry).take(HEAD_BYTES).read_to_end(&mut head).map_err(|e| bad(e.to_string()))?;
        heads.push((path, head, executable));
    }
    let meta = metadata.ok_or_else(|| bad(tr("metadata.json is missing")))?;
    let json: serde_json::Value =
        serde_json::from_slice(&meta).map_err(|_| bad(tr("metadata.json is not valid JSON")))?;
    let uuid = json.get("uuid").and_then(|v| v.as_str()).unwrap_or("");
    if uuid != expected_uuid {
        return Err(bad(trf("the archive belongs to another extension (%1)", &[&uuid])));
    }
    let files: Vec<ArchiveFile> =
        heads.iter().map(|(p, h, x)| ArchiveFile { path: p, head: h, executable: *x }).collect();
    let executables = scan_executables(&files);
    if let Some(bin) = executables.iter().find(|f| f.class == ExecClass::Binary) {
        return Err(bad(trf("the archive contains a program file: %1", &[&bin.path])));
    }
    Ok(ZipCheck { uuid: uuid.to_string(), executables })
}

/// The user extension directory.
pub fn user_extension_dir(data_home: &str, uuid: &str) -> PathBuf {
    Path::new(data_home).join("gnome-shell/extensions").join(uuid)
}

/// Install a downloaded release zip without the shell: archive check →
/// `gnome-extensions install --force` (no utility — unpacking into the uuid
/// directory) → enabling via the script
/// [`crate::de_customization::gnome_extension_enable_script`]. It will start
/// working on the next GNOME login ([`next_login_note`]).
pub fn install_zip_offline(
    io: &mut dyn ShellExtIo,
    zip_path: &Path,
    uuid: &str,
    data_home: &str,
) -> Result<InstallOutcome, ShellExtError> {
    check_extension_zip(zip_path, uuid)?;
    let dest = user_extension_dir(data_home, uuid);
    if io.which(GNOME_EXTENSIONS_BIN) {
        let argv = [
            GNOME_EXTENSIONS_BIN.to_string(),
            "install".into(),
            "--force".into(),
            zip_path.to_string_lossy().into_owned(),
        ];
        match io.run(&argv) {
            Ok(out) if out.success => {}
            Ok(out) => return Err(ShellExtError::Offline(out.text.trim().to_string())),
            Err(e) => return Err(ShellExtError::Offline(e)),
        }
    } else {
        unpack_into(zip_path, &dest)?;
    }
    let script = crate::de_customization::gnome_extension_enable_script(uuid, &dest.to_string_lossy());
    match io.run(&["sh".to_string(), "-c".into(), script]) {
        Ok(out) if out.success => Ok(InstallOutcome::InstalledNextLogin),
        Ok(out) => Err(ShellExtError::Offline(out.text.trim().to_string())),
        Err(e) => Err(ShellExtError::Offline(e)),
    }
}

/// Unpack into a temporary directory next to it and replace the old one
/// entirely: a failure in the middle of unpacking will not leave half an
/// extension.
fn unpack_into(zip_path: &Path, dest: &Path) -> Result<(), ShellExtError> {
    let off = |e: String| ShellExtError::Offline(e);
    let parent = dest.parent().ok_or_else(|| off(dest.display().to_string()))?;
    std::fs::create_dir_all(parent).map_err(|e| off(e.to_string()))?;
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = parent.join(format!(".{name}.rexon-new"));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = crate::ext_utils::safe_zip_extract(zip_path, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(off(e.to_string()));
    }
    if dest.exists() && !crate::ext_utils::safe_rmtree(dest) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(off(dest.display().to_string()));
    }
    std::fs::rename(&staging, dest).map_err(|e| off(e.to_string()))
}

// ── Live bus ─────────────────────────────────────────────────────────────

/// Live connection: the session bus via zbus (synchronous; do not call from the
/// interface thread), settings — `gsettings`, commands — `std::process`.
#[derive(Default)]
pub struct LiveShell {
    conn: Option<zbus::blocking::Connection>,
}

impl LiveShell {
    pub fn new() -> Self {
        LiveShell { conn: None }
    }

    fn connect(timeout: Duration) -> Result<zbus::blocking::Connection, BusFailure> {
        zbus::blocking::connection::Builder::session()
            .map(|b| b.method_timeout(timeout))
            .and_then(|b| b.build())
            .map_err(|e| {
                crate::log_warn!("gnome-ext", "сеансовая шина недоступна: {e}");
                BusFailure::NoShell
            })
    }

    fn conn(&mut self) -> Result<&zbus::blocking::Connection, BusFailure> {
        if self.conn.is_none() {
            self.conn = Some(Self::connect(CALL_TIMEOUT)?);
        }
        Ok(self.conn.as_ref().expect("соединение открыто"))
    }

    fn call<B, R>(&mut self, method: &str, body: &B) -> Result<R, BusFailure>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        let conn = self.conn()?;
        call_on(conn, BUS_NAME, OBJECT_PATH, INTERFACE, method, body)
    }

    fn property(&mut self, name: &str) -> Result<zbus::zvariant::OwnedValue, BusFailure> {
        let conn = self.conn()?;
        call_on(conn, BUS_NAME, OBJECT_PATH, "org.freedesktop.DBus.Properties", "Get", &(INTERFACE, name))
    }
}

fn call_on<B, R>(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> Result<R, BusFailure>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let reply = conn.call_method(Some(dest), path, Some(iface), method, body).map_err(bus_failure)?;
    reply.body().deserialize::<R>().map_err(|e| BusFailure::Other(e.to_string()))
}

/// zbus error → [`BusFailure`].
fn bus_failure(e: zbus::Error) -> BusFailure {
    match e {
        zbus::Error::MethodError(name, detail, _) => {
            let name = name.as_str().to_string();
            match name.as_str() {
                "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner" => {
                    BusFailure::NoShell
                }
                "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => BusFailure::Timeout,
                _ => BusFailure::Method { message: detail.unwrap_or_else(|| name.clone()), name },
            }
        }
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => BusFailure::Timeout,
        other => BusFailure::Other(other.to_string()),
    }
}

/// zvariant → [`Field`].
fn field_of(v: &zbus::zvariant::Value<'_>) -> Field {
    use zbus::zvariant::Value as V;
    match v {
        V::Value(inner) => field_of(inner),
        V::Str(s) => Field::Str(s.as_str().to_string()),
        V::ObjectPath(p) => Field::Str(p.as_str().to_string()),
        V::Bool(b) => Field::Bool(*b),
        V::F64(x) => Field::Num(*x),
        V::U8(x) => Field::Num(f64::from(*x)),
        V::I16(x) => Field::Num(f64::from(*x)),
        V::U16(x) => Field::Num(f64::from(*x)),
        V::I32(x) => Field::Num(f64::from(*x)),
        V::U32(x) => Field::Num(f64::from(*x)),
        V::I64(x) => Field::Num(*x as f64),
        V::U64(x) => Field::Num(*x as f64),
        V::Array(a) => Field::List(
            a.inner()
                .iter()
                .filter_map(|e| match field_of(e) {
                    Field::Str(s) => Some(s),
                    Field::Num(n) => Some(format_number(n)),
                    _ => None,
                })
                .collect(),
        ),
        V::Dict(d) => Field::Map(
            d.iter()
                .filter_map(|(k, v)| match (field_of(k), field_of(v)) {
                    (Field::Str(k), Field::Str(v)) => Some((k, v)),
                    _ => None,
                })
                .collect(),
        ),
        _ => Field::Other,
    }
}

fn raw_of(map: std::collections::HashMap<String, zbus::zvariant::OwnedValue>) -> RawInfo {
    map.into_iter().map(|(k, v)| (k, field_of(&v))).collect()
}

impl ShellExtIo for LiveShell {
    fn shell_running(&mut self) -> bool {
        let Ok(conn) = self.conn() else { return false };
        call_on::<_, bool>(
            conn,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            &(BUS_NAME,),
        )
        .unwrap_or(false)
    }

    fn list(&mut self) -> Result<BTreeMap<String, RawInfo>, BusFailure> {
        type Reply = std::collections::HashMap<String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>>;
        let r: Reply = self.call("ListExtensions", &())?;
        Ok(r.into_iter().map(|(k, v)| (k, raw_of(v))).collect())
    }

    fn info(&mut self, uuid: &str) -> Result<RawInfo, BusFailure> {
        let r: std::collections::HashMap<String, zbus::zvariant::OwnedValue> = self.call("GetExtensionInfo", &(uuid,))?;
        Ok(raw_of(r))
    }

    fn errors(&mut self, uuid: &str) -> Result<Vec<String>, BusFailure> {
        self.call("GetExtensionErrors", &(uuid,))
    }

    fn install_remote(&mut self, uuid: &str, timeout: Duration) -> Result<String, BusFailure> {
        // A separate connection with a long timeout: the shared one stays fast.
        let conn = Self::connect(timeout)?;
        call_on(&conn, BUS_NAME, OBJECT_PATH, INTERFACE, "InstallRemoteExtension", &(uuid,))
    }

    fn uninstall(&mut self, uuid: &str) -> Result<bool, BusFailure> {
        self.call("UninstallExtension", &(uuid,))
    }

    fn enable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
        self.call("EnableExtension", &(uuid,))
    }

    fn disable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
        self.call("DisableExtension", &(uuid,))
    }

    fn open_prefs(&mut self, uuid: &str, parent_window: &str, modal: bool) -> Result<(), BusFailure> {
        let mut options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>> = std::collections::HashMap::new();
        options.insert("modal", zbus::zvariant::Value::from(modal));
        self.call("OpenExtensionPrefs", &(uuid, parent_window, options))
    }

    fn launch_prefs(&mut self, uuid: &str) -> Result<(), BusFailure> {
        self.call("LaunchExtensionPrefs", &(uuid,))
    }

    fn check_for_updates(&mut self) -> Result<(), BusFailure> {
        self.call("CheckForUpdates", &())
    }

    fn shell_version(&mut self) -> Result<String, BusFailure> {
        let v = self.property("ShellVersion")?;
        match field_of(&v) {
            Field::Str(s) => Ok(s),
            other => Err(BusFailure::Other(format!("ShellVersion: {other:?}"))),
        }
    }

    fn user_extensions_enabled(&mut self) -> Result<bool, BusFailure> {
        let v = self.property("UserExtensionsEnabled")?;
        match field_of(&v) {
            Field::Bool(b) => Ok(b),
            other => Err(BusFailure::Other(format!("UserExtensionsEnabled: {other:?}"))),
        }
    }

    fn set_user_extensions_enabled(&mut self, on: bool) -> Result<(), BusFailure> {
        let conn = self.conn()?;
        call_on(
            conn,
            BUS_NAME,
            OBJECT_PATH,
            "org.freedesktop.DBus.Properties",
            "Set",
            &(INTERFACE, "UserExtensionsEnabled", zbus::zvariant::Value::from(on)),
        )
    }

    fn shell_setting_bool(&mut self, key: &str) -> Option<bool> {
        let out = std::process::Command::new("gsettings").args(["get", SHELL_SCHEMA, key]).output().ok()?;
        match String::from_utf8_lossy(&out.stdout).trim() {
            "true" if out.status.success() => Some(true),
            "false" if out.status.success() => Some(false),
            _ => None,
        }
    }

    fn which(&mut self, cmd: &str) -> bool {
        crate::utils::which(cmd, std::env::var("PATH").ok().as_deref()).is_some()
    }

    fn run(&mut self, argv: &[String]) -> Result<CmdOutput, String> {
        let (prog, args) = argv.split_first().ok_or_else(|| "пустая команда".to_string())?;
        let out = std::process::Command::new(prog).args(args).output().map_err(|e| format!("{prog}: {e}"))?;
        let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stdout));
        Ok(CmdOutput { success: out.status.success(), text })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fake shell: dictionaries, answers and a call log.
    #[derive(Default)]
    struct FakeShell {
        running: bool,
        exts: BTreeMap<String, RawInfo>,
        install_reply: Option<Result<String, BusFailure>>,
        action_ok: bool,
        user_enabled: bool,
        allow_install: Option<bool>,
        disable_user: Option<bool>,
        open_fails: bool,
        launch_fails: bool,
        have_tool: bool,
        run_ok: bool,
        calls: Vec<String>,
    }

    impl FakeShell {
        fn up() -> Self {
            FakeShell { running: true, action_ok: true, user_enabled: true, run_ok: true, ..Default::default() }
        }
        fn with(mut self, raw: RawInfo) -> Self {
            let uuid = field_str(&raw, "uuid");
            self.exts.insert(uuid, raw);
            self
        }
    }

    impl ShellExtIo for FakeShell {
        fn shell_running(&mut self) -> bool {
            self.running
        }
        fn list(&mut self) -> Result<BTreeMap<String, RawInfo>, BusFailure> {
            self.calls.push("list".into());
            Ok(self.exts.clone())
        }
        fn info(&mut self, uuid: &str) -> Result<RawInfo, BusFailure> {
            self.calls.push(format!("info {uuid}"));
            Ok(self.exts.get(uuid).cloned().unwrap_or_default())
        }
        fn errors(&mut self, uuid: &str) -> Result<Vec<String>, BusFailure> {
            Ok(vec![format!("TypeError in {uuid}")])
        }
        fn install_remote(&mut self, uuid: &str, timeout: Duration) -> Result<String, BusFailure> {
            self.calls.push(format!("install {uuid} {}", timeout.as_secs()));
            self.install_reply.clone().unwrap_or(Ok(INSTALL_SUCCESSFUL.into()))
        }
        fn uninstall(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("uninstall {uuid}"));
            Ok(self.action_ok)
        }
        fn enable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("enable {uuid}"));
            Ok(self.action_ok)
        }
        fn disable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("disable {uuid}"));
            Ok(self.action_ok)
        }
        fn open_prefs(&mut self, uuid: &str, _p: &str, _m: bool) -> Result<(), BusFailure> {
            self.calls.push(format!("open {uuid}"));
            if self.open_fails {
                Err(BusFailure::Method { name: "org.freedesktop.DBus.Error.UnknownMethod".into(), message: "x".into() })
            } else {
                Ok(())
            }
        }
        fn launch_prefs(&mut self, uuid: &str) -> Result<(), BusFailure> {
            self.calls.push(format!("launch {uuid}"));
            if self.launch_fails { Err(BusFailure::Other("нет".into())) } else { Ok(()) }
        }
        fn check_for_updates(&mut self) -> Result<(), BusFailure> {
            self.calls.push("updates".into());
            Ok(())
        }
        fn shell_version(&mut self) -> Result<String, BusFailure> {
            Ok("51.1".into())
        }
        fn user_extensions_enabled(&mut self) -> Result<bool, BusFailure> {
            Ok(self.user_enabled)
        }
        fn set_user_extensions_enabled(&mut self, on: bool) -> Result<(), BusFailure> {
            self.user_enabled = on;
            Ok(())
        }
        fn shell_setting_bool(&mut self, key: &str) -> Option<bool> {
            match key {
                ALLOW_INSTALL_KEY => self.allow_install,
                DISABLE_USER_KEY => self.disable_user,
                _ => None,
            }
        }
        fn which(&mut self, cmd: &str) -> bool {
            cmd == GNOME_EXTENSIONS_BIN && self.have_tool
        }
        fn run(&mut self, argv: &[String]) -> Result<CmdOutput, String> {
            self.calls.push(format!("run {}", argv[..argv.len().min(3)].join(" ")));
            Ok(CmdOutput { success: self.run_ok, text: if self.run_ok { String::new() } else { "сбой\n".into() } })
        }
    }

    #[test]
    fn shell_version_from_bus_or_command() {
        // The shell is running — the version comes from the bus.
        assert_eq!(detect_shell_version(&mut FakeShell::up()).as_deref(), Some("51.1"));
        // No shell and no gnome-shell program — there is no GNOME here.
        let mut none = FakeShell { running: false, ..FakeShell::up() };
        assert_eq!(detect_shell_version(&mut none), None);
        // Output of `gnome-shell --version`, including with stderr noise.
        assert_eq!(version_from_output("GNOME Shell 51.1
").as_deref(), Some("51.1"));
        assert_eq!(
            version_from_output("Gjs-WARNING: что-то
GNOME Shell 50.beta
").as_deref(),
            Some("50.beta")
        );
        assert_eq!(version_from_output("GNOME Shell
"), None);
        assert_eq!(version_from_output(""), None);
    }

    /// A synthetic dictionary in the form GNOME 51 returns it
    /// (numbers are double, `shell-version` is av).
    fn raw(uuid: &str, state: f64, kind: f64) -> RawInfo {
        let mut m = RawInfo::new();
        m.insert("uuid".into(), Field::Str(uuid.into()));
        m.insert("name".into(), Field::Str("Sample".into()));
        m.insert("description".into(), Field::Str("Synthetic extension".into()));
        m.insert("state".into(), Field::Num(state));
        m.insert("type".into(), Field::Num(kind));
        m.insert("enabled".into(), Field::Bool(state == 1.0));
        m.insert("path".into(), Field::Str(format!("/home/u/.local/share/gnome-shell/extensions/{uuid}")));
        m.insert("error".into(), Field::Str(String::new()));
        m.insert("hasPrefs".into(), Field::Bool(true));
        m.insert("hasUpdate".into(), Field::Bool(false));
        m.insert("canChange".into(), Field::Bool(true));
        m.insert("version".into(), Field::Num(6.0));
        m.insert("shell-version".into(), Field::List(vec!["50".into(), "51".into()]));
        m.insert("sessionModes".into(), Field::List(vec!["user".into()]));
        m
    }

    fn set(mut m: RawInfo, key: &str, v: Field) -> RawInfo {
        m.insert(key.into(), v);
        m
    }

    const U: &str = "sample@example.org";

    #[test]
    fn parses_shell_dictionary() {
        let mut r = raw(U, 1.0, 2.0);
        r.insert("version-name".into(), Field::Str("0.4.5".into()));
        r.insert("donations".into(), Field::Map([("github".to_string(), "someone".to_string())].into()));
        let e = ExtInfo::from_raw(&r).unwrap();
        assert_eq!(e.uuid, U);
        assert_eq!(e.state, ExtState::Active);
        assert!(e.enabled && e.has_prefs && e.can_change && !e.is_system && !e.has_update);
        assert_eq!(e.version, "6");
        assert_eq!(e.display_version(), "0.4.5");
        assert_eq!(e.shell_versions, vec!["50", "51"]);
        assert_eq!(e.session_modes, vec!["user"]);
        assert_eq!(e.donations.get("github").map(String::as_str), Some("someone"));
    }

    #[test]
    fn empty_dictionary_is_not_installed() {
        assert_eq!(ExtInfo::from_raw(&RawInfo::new()), None);
        let mut io = FakeShell::up();
        assert_eq!(extension_info(&mut io, U).unwrap(), None);
        assert_eq!(set_enabled(&mut io, U, true), Err(ShellExtError::NotInstalled));
    }

    #[test]
    fn tolerant_defaults_for_old_shells() {
        let mut r = RawInfo::new();
        r.insert("uuid".into(), Field::Str(U.into()));
        r.insert("state".into(), Field::Num(1.0));
        r.insert("session-modes".into(), Field::List(vec!["unlock-dialog".into()]));
        let e = ExtInfo::from_raw(&r).unwrap();
        assert_eq!(e.name, U, "без имени — uuid");
        assert!(e.enabled, "без enabled — по состоянию");
        assert!(e.can_change, "без canChange — менять можно");
        assert_eq!(e.version, "");
        assert_eq!(e.display_version(), "");
        assert_eq!(e.session_modes, vec!["unlock-dialog"]);
    }

    #[test]
    fn states_round_trip() {
        for code in [1, 2, 3, 4, 5, 6, 7, 8, 99, 42] {
            assert_eq!(ExtState::from_code(code).code(), code);
        }
        assert!(ExtState::Error.is_problem() && ExtState::OutOfDate.is_problem());
        assert!(!ExtState::Active.is_problem());
        assert_eq!(ExtState::from_code(4).key(), "out-of-date");
        let e = ExtInfo::from_raw(&set(raw(U, 3.0, 2.0), "error", Field::Str("boom".into()))).unwrap();
        assert_eq!((e.state, e.error.as_str(), e.enabled), (ExtState::Error, "boom", false));
        assert!(ExtInfo::from_raw(&raw(U, 1.0, 1.0)).unwrap().is_system);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(6.0), "6");
        assert_eq!(format_number(2.5), "2.5");
    }

    #[test]
    fn list_and_pending_updates() {
        let mut io = FakeShell::up()
            .with(raw(U, 1.0, 2.0))
            .with(set(raw("b@example.org", 2.0, 2.0), "hasUpdate", Field::Bool(true)));
        io.exts.insert("junk".into(), RawInfo::new());
        let list = list_extensions(&mut io).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(pending_updates(&list), 1);
        assert_eq!(extension_errors(&mut io, U).unwrap(), vec![format!("TypeError in {U}")]);
        check_for_updates(&mut io).unwrap();
        assert!(io.calls.contains(&"updates".to_string()));
        assert_eq!(shell_version(&mut io).unwrap(), "51.1");
    }

    #[test]
    fn no_shell_is_explained() {
        let mut io = FakeShell { running: false, ..FakeShell::up() };
        assert_eq!(list_extensions(&mut io), Err(ShellExtError::ShellNotRunning));
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::ShellNotRunning));
        assert!(io.calls.is_empty(), "на шину не ходили");
        assert!(ShellExtError::ShellNotRunning.message().contains("isn't running"));
    }

    #[test]
    fn install_successful_and_cancelled() {
        let mut io = FakeShell::up();
        assert_eq!(install_remote(&mut io, U), Ok(InstallOutcome::Installed));
        assert_eq!(io.calls, vec![format!("install {U} {}", INSTALL_TIMEOUT.as_secs())]);
        assert!(INSTALL_TIMEOUT >= Duration::from_secs(120), "ждём минуты, а не секунды");
        io.install_reply = Some(Ok(INSTALL_CANCELLED.into()));
        assert_eq!(install_remote(&mut io, U), Ok(InstallOutcome::Cancelled), "«Отмена» — не ошибка");
        io.install_reply = Some(Ok("weird".into()));
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::Bus("weird".into())));
    }

    #[test]
    fn install_refusals() {
        let mut io = FakeShell { allow_install: Some(false), ..FakeShell::up() };
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::InstallForbidden));
        assert!(io.calls.is_empty(), "запрет виден до вызова");
        assert!(ShellExtError::InstallForbidden.message().contains("forbidden"));

        let mut io = FakeShell::up();
        io.install_reply = Some(Err(BusFailure::Timeout));
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::Timeout));
        io.install_reply = Some(Err(BusFailure::Method { name: "org.gnome.Shell.ExtensionError".into(), message: "404".into() }));
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::Bus("404".into())));
        io.disable_user = Some(true);
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::UserExtensionsDisabled));
        io.install_reply = Some(Err(BusFailure::NoShell));
        assert_eq!(install_remote(&mut io, U), Err(ShellExtError::ShellNotRunning), "оболочка ушла посреди");

        assert_eq!(install_remote(&mut io, "../evil"), Err(ShellExtError::BadUuid("../evil".into())));
    }

    #[test]
    fn uninstall_rules() {
        let mut io = FakeShell::up().with(raw(U, 1.0, 2.0)).with(raw("sys@example.org", 1.0, 1.0));
        assert_eq!(uninstall(&mut io, U), Ok(()));
        assert_eq!(uninstall(&mut io, "sys@example.org"), Err(ShellExtError::SystemExtension));
        assert_eq!(uninstall(&mut io, "none@example.org"), Err(ShellExtError::NotInstalled));
        io.action_ok = false;
        assert_eq!(uninstall(&mut io, U), Err(ShellExtError::Refused));
        assert_eq!(io.calls.iter().filter(|c| c.starts_with("uninstall")).count(), 2);
    }

    #[test]
    fn enable_disable_rules() {
        let mut io = FakeShell::up().with(raw(U, 2.0, 2.0)).with(set(raw("lock@example.org", 1.0, 2.0), "canChange", Field::Bool(false)));
        assert_eq!(set_enabled(&mut io, U, true), Ok(()));
        assert_eq!(set_enabled(&mut io, U, false), Ok(()));
        assert_eq!(set_enabled(&mut io, "lock@example.org", false), Err(ShellExtError::Locked));
        io.user_enabled = false;
        assert_eq!(set_enabled(&mut io, U, true), Err(ShellExtError::UserExtensionsDisabled));
        assert_eq!(set_enabled(&mut io, U, false), Ok(()), "выключить можно и так");
        io.user_enabled = true;
        io.action_ok = false;
        assert_eq!(set_enabled(&mut io, U, true), Err(ShellExtError::Refused));
        io.disable_user = Some(true);
        assert_eq!(set_enabled(&mut io, U, true), Err(ShellExtError::UserExtensionsDisabled));
        assert!(ShellExtError::UserExtensionsDisabled.message().contains("turned off"));
    }

    #[test]
    fn user_extensions_switch() {
        let mut io = FakeShell::up();
        assert_eq!(user_extensions_enabled(&mut io), Ok(true));
        set_user_extensions_enabled(&mut io, false).unwrap();
        assert_eq!(user_extensions_enabled(&mut io), Ok(false));
    }

    #[test]
    fn prefs_fallback_chain() {
        let mut io = FakeShell::up().with(raw(U, 1.0, 2.0));
        open_prefs(&mut io, U, "").unwrap();
        assert_eq!(io.calls.last().unwrap(), &format!("open {U}"));

        io.open_fails = true;
        io.calls.clear();
        open_prefs(&mut io, U, "").unwrap();
        assert_eq!(io.calls, vec![format!("info {U}"), format!("open {U}"), format!("launch {U}")]);

        io.launch_fails = true;
        assert_eq!(open_prefs(&mut io, U, ""), Err(ShellExtError::Bus("нет".into())), "утилиты нет — последняя ошибка");
        io.have_tool = true;
        io.calls.clear();
        open_prefs(&mut io, U, "").unwrap();
        assert_eq!(io.calls.last().unwrap(), &format!("run gnome-extensions prefs {U}"));

        let mut io = FakeShell::up().with(set(raw(U, 1.0, 2.0), "hasPrefs", Field::Bool(false)));
        assert_eq!(open_prefs(&mut io, U, ""), Err(ShellExtError::NoPrefs));

        let mut io = FakeShell { running: false, have_tool: true, ..FakeShell::up() };
        open_prefs(&mut io, U, "").unwrap();
        assert_eq!(io.calls, vec![format!("run gnome-extensions prefs {U}")]);
        let mut io = FakeShell { running: false, ..FakeShell::up() };
        assert_eq!(open_prefs(&mut io, U, ""), Err(ShellExtError::ShellNotRunning));
    }

    // ── release zip ──

    fn tmp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rexon_gse_{tag}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn make_zip(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("ext.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        path
    }

    fn meta(uuid: &str) -> Vec<u8> {
        format!(r#"{{"uuid": "{uuid}", "name": "Sample", "shell-version": ["51"]}}"#).into_bytes()
    }

    #[test]
    fn zip_check_accepts_plain_extension() {
        let d = tmp_dir("ok");
        let m = meta(U);
        let z = make_zip(&d, &[("metadata.json", &m), ("extension.js", b"export default class {}"), ("schemas/a.xml", b"<x/>")]);
        let c = check_extension_zip(&z, U).unwrap();
        assert_eq!(c.uuid, U);
        assert_eq!(c.executables.len(), 1, "JavaScript найден, но он — норма");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn zip_check_rejects() {
        let d = tmp_dir("bad");
        let m = meta(U);
        let cases: Vec<Vec<(&str, &[u8])>> = vec![
            vec![("metadata.json", &m), ("../evil.js", b"x")],
            vec![("metadata.json", &m), ("/abs.js", b"x")],
            vec![("metadata.json", &m), ("bin/tool", b"\x7fELF\x02\x01")],
            vec![("extension.js", b"x")],
            vec![("metadata.json", b"{not json")],
            vec![("metadata.json", br#"{"uuid": "other@example.org"}"#)],
        ];
        for files in cases {
            let z = make_zip(&d, &files);
            assert!(
                matches!(check_extension_zip(&z, U), Err(ShellExtError::BadArchive(_))),
                "{:?}",
                files.iter().map(|f| f.0).collect::<Vec<_>>()
            );
        }
        std::fs::write(d.join("ext.zip"), b"not a zip").unwrap();
        assert!(matches!(check_extension_zip(&d.join("ext.zip"), U), Err(ShellExtError::BadArchive(_))));
        assert_eq!(check_extension_zip(&d.join("ext.zip"), "bad"), Err(ShellExtError::BadUuid("bad".into())));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn offline_install_via_tool() {
        let d = tmp_dir("tool");
        let m = meta(U);
        let z = make_zip(&d, &[("metadata.json", &m), ("extension.js", b"x")]);
        let mut io = FakeShell { running: false, have_tool: true, ..FakeShell::up() };
        let data = d.join("data");
        assert_eq!(install_zip_offline(&mut io, &z, U, &data.to_string_lossy()), Ok(InstallOutcome::InstalledNextLogin));
        assert_eq!(io.calls, vec!["run gnome-extensions install --force".to_string(), "run sh -c meta=".to_string() + &enable_head(&data)]);
        assert!(!data.exists(), "файлы кладёт утилита, не мы");

        io.run_ok = false;
        assert_eq!(install_zip_offline(&mut io, &z, U, &data.to_string_lossy()), Err(ShellExtError::Offline("сбой".into())));
        assert!(next_login_note().contains("next time"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Start of the third word in the `run sh -c <script>` log — up to the line break.
    fn enable_head(data: &Path) -> String {
        let s = crate::de_customization::gnome_extension_enable_script(U, &user_extension_dir(&data.to_string_lossy(), U).to_string_lossy());
        s.strip_prefix("meta=").unwrap().to_string()
    }

    #[test]
    fn offline_install_by_unpacking() {
        let d = tmp_dir("unpack");
        let m = meta(U);
        let z = make_zip(&d, &[("metadata.json", &m), ("extension.js", b"new")]);
        let data = d.join("data");
        let dest = user_extension_dir(&data.to_string_lossy(), U);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("stale.js"), b"old").unwrap();
        let mut io = FakeShell { running: false, ..FakeShell::up() };
        assert_eq!(install_zip_offline(&mut io, &z, U, &data.to_string_lossy()), Ok(InstallOutcome::InstalledNextLogin));
        assert_eq!(std::fs::read(dest.join("extension.js")).unwrap(), b"new");
        assert!(!dest.join("stale.js").exists(), "прежний каталог заменён целиком");
        assert!(!dest.parent().unwrap().join(format!(".{U}.rexon-new")).exists());
        assert_eq!(io.calls.len(), 1, "только сценарий включения");
        assert!(io.calls[0].starts_with("run sh -c"));

        let bad = make_zip(&d, &[("metadata.json", &m), ("../x", b"x")]);
        assert!(matches!(install_zip_offline(&mut io, &bad, U, &data.to_string_lossy()), Err(ShellExtError::BadArchive(_))));
        assert_eq!(std::fs::read(dest.join("extension.js")).unwrap(), b"new", "плохой архив ничего не тронул");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn messages_are_distinct() {
        let all = [
            ShellExtError::ShellNotRunning,
            ShellExtError::InstallForbidden,
            ShellExtError::UserExtensionsDisabled,
            ShellExtError::Locked,
            ShellExtError::SystemExtension,
            ShellExtError::NotInstalled,
            ShellExtError::NoPrefs,
            ShellExtError::BadUuid("x".into()),
            ShellExtError::Timeout,
            ShellExtError::Refused,
            ShellExtError::Bus("x".into()),
            ShellExtError::BadArchive("x".into()),
            ShellExtError::Offline("x".into()),
        ];
        let texts: std::collections::BTreeSet<String> = all.iter().map(|e| e.message()).collect();
        assert_eq!(texts.len(), all.len());
    }

    #[test]
    fn zvariant_conversion() {
        use zbus::zvariant::Value;
        assert_eq!(field_of(&Value::from(6.0f64)), Field::Num(6.0));
        assert_eq!(field_of(&Value::Value(Box::new(Value::from("x")))), Field::Str("x".into()));
        let av = Value::from(vec![Value::Value(Box::new(Value::from("50"))), Value::Value(Box::new(Value::from("51")))]);
        assert_eq!(field_of(&av), Field::List(vec!["50".into(), "51".into()]));
        assert_eq!(field_of(&Value::from(vec!["user", "unlock-dialog"])), Field::List(vec!["user".into(), "unlock-dialog".into()]));
        assert_eq!(field_of(&Value::from(true)), Field::Bool(true));
    }
}
