//! Port of the pure part of the Qt entry points — `rexon/qt_main/app.py` and
//! four small ones (`qt_extensions/app.py`, `qt_settings/app.py`,
//! `qt_desktops/app.py`, `qt_compare/app.py`).
//!
//! Flag routing and the KDE emergency rollback live in [`crate::app`] (port of
//! `rexon/app.py`) — what moved here is what is not there: the **crash report**
//! and the **Qt application identity**.
//!
//! BOUNDARY. The core holds decisions, texts and formats:
//!   * the report header, the file name, directory rotation, the dialog
//!     contents, the "print/save/show" decision by exception type and thread;
//!   * the table of `applicationName`/`applicationDisplayName`/
//!     `organizationName`/`desktopFileName` for each entry point.
//!
//! Execution stays behind the Qt layer: the `QApplication` itself and the setter
//! calls, `QMessageBox` (building the window from [`CrashDialog`]), the
//! clipboard, launching `xdg-open`, `faulthandler`/`SIGUSR1`, the guard against
//! SEGV of native dialogs (KDE 521894), the Qt message logger, the skin and the
//! transparent window icon. The core does not write to disk: the report's file
//! operations go through the [`CrashIo`] trait (like `ImpIo` in `importers`), so
//! tests do not touch the real file system.
//!
//! PITFALLS OF THE ORIGINAL ARE FIXED (rule #16, each marked `Fix:` in place):
//!   1. `_crash_header()` was called TWICE — separately for the file and
//!      separately for the dialog, so the `Time:` line in the saved log and in
//!      the window text diverged (and those are what a buyer's report is later
//!      matched to the log by); here the header is built once and passed to
//!      both.
//!   2. `SystemExit` was not a "silent" exception. In a worker thread
//!      `sys.exit()` reaches `threading.excepthook` (checked on CPython 3.14),
//!      and a regular worker exit showed the buyer "Rexon quit unexpectedly" and
//!      wrote a crash report; here `SystemExit` is treated like
//!      `KeyboardInterrupt`.
//!   3. The dialog was built right in the thread where the exception happened:
//!      `threading.excepthook` → `QMessageBox` outside the GUI thread = a Qt
//!      abort. Showing the dialog is not cancelled (otherwise a Steam buyer would
//!      again see nothing) — the decision carries the flag
//!      [`CrashDecision::dialog_needs_main_thread_hop`], by which the Qt layer
//!      must hand the showing over to the GUI thread.
//!   4. The path of the saved log was lost if walking the directory failed after
//!      a successful write: rotation was inside a shared `try`, and the function
//!      returned `None` with the file already existing — the dialog then said
//!      nothing about the saved report. Here a rotation error does not cancel the
//!      path.
//!
//! The reference is `tests/golden/qt_app_cases.json`, captured by running the
//! LIVE `rexon.qt_main.app`: `_crash_header` with substituted time and
//! environment sources, `_save_crash_log` in a temporary directory,
//! `_show_crash_dialog` and `_install_excepthook` with recording Qt stubs. The
//! check is `tests/qt_app_parity.rs`.

use crate::ext_utils::py_repr_str;
use crate::i18n::{tr, trf};

// ============================================================================
// Qt platform
// ============================================================================

/// Whether `QT_QPA_PLATFORM` must be removed from the environment.
///
/// Under KDE Plasma 6 on Wayland an EMPTY variable prevents Qt from choosing the
/// native wayland plugin, so it is removed. A non-empty value (including `xcb`)
/// is respected: `xcb` must not be forced — KWin does not draw SSD for xcb
/// windows through Xwayland, and the window stays without a title bar and
/// buttons.
pub fn should_unset_qt_platform(value: Option<&str>) -> bool {
    value == Some("")
}

// ============================================================================
// Qt application identity
// ============================================================================

/// The organization — shared by all entry points.
pub const ORGANIZATION_NAME: &str = "OpenAxon";

/// Name of the `.desktop` file without the extension. KDE Plasma 6 / Wayland
/// uses it to associate the process with `/usr/share/applications/rexon.desktop`
/// and takes the taskbar icon from there; without it the app_id is empty and
/// Plasma draws the default Wayland logo.
pub const DESKTOP_FILE_NAME: &str = "rexon";

/// A script for which the application sets its own fallback font. The list is
/// ours, not `QChar::Script`: the Qt number behind a script is known by the glue
/// layer (`cpp/rexon_gui_factory.cpp`); the core only says "which one".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FallbackScript {
    /// Arabic script — Arabic and Persian.
    Arabic = 0,
    /// Hebrew script — Hebrew.
    Hebrew = 1,
}

/// Fallback fonts for right-to-left scripts. Without them fontconfig gives
/// DejaVu Sans first: its Arabic has no UI-height variants, its Hebrew is
/// incomplete, and the line comes out uneven. The families come from
/// `noto-fonts` (the package dependency is set explicitly, not transitively via
/// `qt6-webengine`). `Noto Sans Arabic UI` rather than `Noto Sans Arabic`: the UI
/// variant has a line height tuned for interfaces and does not stretch lists.
pub const RTL_FALLBACK_FONTS: [(FallbackScript, &str); 2] = [
    (FallbackScript::Arabic, "Noto Sans Arabic UI"),
    (FallbackScript::Hebrew, "Noto Sans Hebrew"),
];

/// What an entry point tells `QApplication` about itself. `None` — the setter is
/// not called at all in the original (this is NOT the same as an empty string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QtIdentity {
    pub application_name: &'static str,
    pub display_name: Option<&'static str>,
    pub organization_name: Option<&'static str>,
    pub desktop_file_name: Option<&'static str>,
}

/// Identity by the name of the entry point's Python module. The keys match the
/// values of [`crate::app::SINGLE_TAB_FLAGS`], so that routing and identity
/// cannot diverge (rule #12).
///
/// The first six are windows with tabs. The other five are wallpaper engine
/// hosts (class D of the plan for leaving PySide6): they set only the
/// application name and `desktopFileName`, and do not touch the organization and
/// the display name.
pub const ENTRY_IDENTITIES: &[(&str, QtIdentity)] = &[
    (
        "rexon.qt_main.app",
        QtIdentity {
            application_name: "Rexon",
            // An empty string — ON PURPOSE, not "unset": KWin/Qt glues the
            // displayName onto the end of the title ("<title> — <displayName>"), and
            // all windows already start with "Rexon — …", so "Rexon" would be
            // duplicated.
            display_name: Some(""),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.appearance_page",
        QtIdentity {
            application_name: "Rexon Appearance",
            display_name: Some("Rexon — Оформление"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_extensions.app",
        QtIdentity {
            application_name: "Rexon Extensions",
            display_name: Some("Rexon — Расширения"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_compare.app",
        QtIdentity {
            application_name: "Rexon Compare",
            display_name: Some("Rexon — Сравнение рабочих столов"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_settings.app",
        QtIdentity {
            application_name: "Rexon Settings",
            display_name: Some("Rexon — Настройки"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.qt_desktops.app",
        QtIdentity {
            application_name: "Rexon Desktops",
            display_name: Some("Rexon — Рабочие столы"),
            organization_name: Some(ORGANIZATION_NAME),
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.web_wallpaper",
        QtIdentity {
            application_name: "RexonWebWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.video_wallpaper",
        QtIdentity {
            application_name: "RexonVideoWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.scene_wallpaper",
        QtIdentity {
            application_name: "RexonSceneWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.app_wallpaper",
        QtIdentity {
            application_name: "RexonAppWallpaper",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.parallax_engine",
        QtIdentity {
            application_name: "RexonSceneEngine",
            display_name: None,
            organization_name: None,
            desktop_file_name: Some(DESKTOP_FILE_NAME),
        },
    ),
    (
        "rexon.wallpaper_mmap",
        QtIdentity {
            application_name: "RexonWallpaperHeadless",
            display_name: None,
            organization_name: None,
            desktop_file_name: None,
        },
    ),
];

/// Identity of an entry point by the name of its Python module.
pub fn identity(module: &str) -> Option<QtIdentity> {
    ENTRY_IDENTITIES
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, id)| *id)
}

// ============================================================================
// Report time
// ============================================================================

/// A moment in time without a time zone — exactly what the original takes from
/// `datetime.datetime.now()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timestamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: u32,
}

impl Timestamp {
    /// LOCAL time right now — port of `datetime.datetime.now()`.
    ///
    /// `None` — the time zone cannot be read (a broken `/etc/localtime`, a trimmed
    /// container). This is NOT a reason to crash: the caller decides what to
    /// replace the mark with.
    ///
    /// Warning: it is LOCAL time, not UTC: panel copies and report files are named
    /// by this mark, and a person looks for them by eye using their own time. There
    /// must be no second call to `localtime_r` in the product (rule #12) —
    /// everything that needs the local clock calls this function.
    pub fn now_local() -> Option<Self> {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        let t = epoch.as_secs() as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return None;
        }
        Some(Timestamp {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            microsecond: epoch.subsec_micros(),
        })
    }

    /// Port of `datetime.isoformat()`: the fractional part is printed ONLY when the
    /// microseconds are non-zero (six digits, without rounding to milliseconds).
    pub fn isoformat(&self) -> String {
        let base = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.microsecond == 0 {
            base
        } else {
            format!("{base}.{:06}", self.microsecond)
        }
    }

    /// Port of `strftime("%Y%m%d_%H%M%S")` — the basis of the report file name. The
    /// format is chosen so that alphabetical sorting of names = sorting by time;
    /// rotation relies on this.
    pub fn file_stamp(&self) -> String {
        format!(
            "{:04}{:02}{:02}_{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

// ============================================================================
// Report header
// ============================================================================

/// Length of the separator line in the header (the `─` character).
pub const CRASH_HEADER_RULE_WIDTH: usize = 60;

/// Header sources. Everything the original reads from `platform`, `PySide6` and
/// the environment comes from outside: the core accesses neither the system nor
/// Qt.
#[derive(Debug, Clone, Default)]
pub struct CrashHeaderInput<'a> {
    pub now: Timestamp,
    /// `platform.python_version()`.
    pub python_version: &'a str,
    /// `PySide6.__version__`; `None` — the import failed (`?` in the header).
    pub pyside_version: Option<&'a str>,
    /// `platform.platform()`.
    pub os_platform: &'a str,
    pub xdg_current_desktop: Option<&'a str>,
    pub desktop_session: Option<&'a str>,
    pub xdg_session_type: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Crash report header — byte for byte as in the original.
///
/// Warning: two fields live by DIFFERENT rules, and this is not a slip of the
/// original: the desktop is taken via `or` (an empty string falls through to
/// `DESKTOP_SESSION`, then to `?`), while the session type and the locale go via
/// `get(..., "?")`, where `?` is substituted only when the variable is ABSENT,
/// and an empty string is printed empty.
pub fn crash_header(input: &CrashHeaderInput) -> String {
    let pyside = input.pyside_version.unwrap_or("?");
    crash_header_with(
        &input.now,
        &[("Python", input.python_version), ("PySide6", pyside)],
        &CrashHeaderEnv {
            os_platform: input.os_platform,
            xdg_current_desktop: input.xdg_current_desktop,
            desktop_session: input.desktop_session,
            xdg_session_type: input.xdg_session_type,
            lang: input.lang,
        },
    )
}

/// Width of a header field label together with the colon and padding:
/// `Time:     `. The values of all fields start in one column — the reports of
/// both entry points read the same by eye.
const CRASH_HEADER_LABEL_WIDTH: usize = 10;

/// Environment shared by the headers of both entry points (the Python host and
/// the Rust window).
#[derive(Debug, Clone, Copy, Default)]
pub struct CrashHeaderEnv<'a> {
    /// The OS line in the form of `platform.platform()` — see [`os_platform`].
    pub os_platform: &'a str,
    pub xdg_current_desktop: Option<&'a str>,
    pub desktop_session: Option<&'a str>,
    pub xdg_session_type: Option<&'a str>,
    pub lang: Option<&'a str>,
}

/// Shared header builder: `runtime` — the "what the process is built with" lines
/// between `Time:` and `OS:` (for the Python host these are Python and PySide6,
/// for the Rust window the Rexon and Qt versions). Everything else is one for
/// both entry points (rule #12).
pub fn crash_header_with(now: &Timestamp, runtime: &[(&str, &str)], env: &CrashHeaderEnv) -> String {
    let de = first_nonempty(&[env.xdg_current_desktop, env.desktop_session]).unwrap_or("?");
    let sess = env.xdg_session_type.unwrap_or("?");
    let lang = env.lang.unwrap_or("?");
    let rule: String = "─".repeat(CRASH_HEADER_RULE_WIDTH);
    let field = |label: &str, value: &str| {
        format!("{:<w$}{value}\n", format!("{label}:"), w = CRASH_HEADER_LABEL_WIDTH)
    };
    let mut out = String::from("Rexon crash report\n");
    out.push_str(&field("Time", &now.isoformat()));
    for (label, value) in runtime {
        out.push_str(&field(label, value));
    }
    out.push_str(&field("OS", env.os_platform));
    out.push_str(&field("DE", &format!("{de} ({sess})")));
    out.push_str(&field("Locale", lang));
    out.push_str(&rule);
    out.push_str("\n\n");
    out
}

/// Header sources of the Rust window's report. Everything read from the system
/// comes from outside (`rexon-gui::crash_report`).
#[derive(Debug, Clone, Default)]
pub struct RustCrashHeaderInput<'a> {
    pub now: Timestamp,
    /// Rexon version (`CARGO_PKG_VERSION` of the window crate).
    pub rexon_version: &'a str,
    /// Name of the executable in which the panic happened.
    pub entry: &'a str,
    /// `qVersion()` of the loaded Qt; `None` — unknown (`?` in the header).
    pub qt_version: Option<&'a str>,
    pub env: CrashHeaderEnv<'a>,
}

/// Header of the Rust window's report: the same fields and the same layout as
/// the Python host's; only the runtime lines differ (`Rexon:`/`Qt:` instead of
/// `Python:`/`PySide6:`).
pub fn rust_crash_header(input: &RustCrashHeaderInput) -> String {
    let rexon = if input.entry.is_empty() {
        input.rexon_version.to_string()
    } else {
        format!("{} ({})", input.rexon_version, input.entry)
    };
    crash_header_with(
        &input.now,
        &[("Rexon", &rexon), ("Qt", input.qt_version.unwrap_or("?"))],
        &input.env,
    )
}

/// The OS line in the form of Python's `platform.platform()` on Linux:
/// `Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44`. One notation for both entry
/// points — so that the `OS:` lines in reports can be compared as is.
///
/// `libc` — `(name, version)`, for example `("glibc", "2.44")`; `None` — there
/// is no `-with-…` tail (musl and others, as in Python).
pub fn os_platform(system: &str, release: &str, machine: &str, libc: Option<(&str, &str)>) -> String {
    let mut parts: Vec<String> = [system, release, machine]
        .iter()
        .map(|s| s.trim().replace(' ', "_"))
        .filter(|s| !s.is_empty())
        .collect();
    if let Some((name, ver)) = libc {
        parts.push("with".to_string());
        parts.push(format!("{name}{ver}"));
    }
    parts.join("-")
}

/// The first non-empty value — port of the `a or b` chain.
fn first_nonempty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .find_map(|v| v.filter(|s| !s.is_empty()))
}

// ============================================================================
// Saving the report and rotating the directory
// ============================================================================

/// How many files remain in the report directory after rotation.
pub const CRASH_KEEP: usize = 20;

/// Prefix of the report file name; candidates for deletion are selected by it
/// too, so rotation does not touch other files in the directory (a README and
/// so on).
pub const CRASH_PREFIX: &str = "crash_";

/// The report directory: `~/.config/rexon/crashes` adjusted for
/// `$XDG_CONFIG_HOME` (the original builds it via `utils.expand_user`).
pub fn crash_dir(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/crashes",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Report file name for the given moment in time.
pub fn crash_log_name(ts: &Timestamp) -> String {
    format!("{CRASH_PREFIX}{}.log", ts.file_stamp())
}

/// Which names go away on rotation: from the sorted list of reports everything
/// except the last `keep` is thrown out. Files without the prefix are not
/// considered at all.
pub fn rotation_victims(names: &[String], keep: usize) -> Vec<String> {
    let mut files: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(CRASH_PREFIX))
        .cloned()
        .collect();
    files.sort();
    let cut = files.len().saturating_sub(keep);
    files.truncate(cut);
    files
}

/// File operations of the report. Each method is exactly one side-effecting call
/// of the original, in execution order; `false`/`None` = the exception was
/// caught (in Python the whole block is wrapped in `try/except Exception`).
pub trait CrashIo {
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str) -> bool;
    /// `open(path, "w", encoding="utf-8").write(text)`.
    fn write_text(&mut self, path: &str, text: &str) -> bool;
    /// `os.listdir(path)`.
    fn list_dir(&mut self, path: &str) -> Option<Vec<String>>;
    /// `os.remove(path)`; the original itself swallows the error.
    fn remove(&mut self, path: &str);
}

/// Save the report with rotation. Returns the path to the file, or `None` if the
/// directory was not created or the write failed.
///
/// `header` comes from outside and goes to the dialog with the same value — see
/// fix No. 1 in the module header.
///
/// Fix No. 4: an error walking the directory no longer cancels the already
/// written path.
pub fn save_crash_log(
    io: &mut dyn CrashIo,
    dir: &str,
    ts: &Timestamp,
    header: &str,
    crash_text: &str,
) -> Option<String> {
    if !io.makedirs(dir) {
        return None;
    }
    let path = format!("{dir}/{}", crash_log_name(ts));
    if !io.write_text(&path, &format!("{header}{crash_text}")) {
        return None;
    }
    if let Some(names) = io.list_dir(dir) {
        for victim in rotation_victims(&names, CRASH_KEEP) {
            io.remove(&format!("{dir}/{victim}"));
        }
    }
    Some(path)
}

/// The live [`CrashIo`] executor — ONE for both entry points (the Python bridge
/// and the Rust window), so it lives here rather than in each of them.
///
/// The write is atomic: the text goes into a temporary file next to it
/// (`.<name>.part-<pid>`, without the [`CRASH_PREFIX`] prefix — rotation does
/// not see it), is flushed to disk and only then renamed to the final name. The
/// report is written at the moment of the crash, and the process may die in the
/// middle of the write (a panic across the FFI boundary is an `abort` right
/// after the hook): with an in-place write a truncated `crash_*.log` that looks
/// like a real one would remain in the directory. Here a file with such a name
/// is either complete or absent.
pub struct AtomicCrashIo;

impl CrashIo for AtomicCrashIo {
    fn makedirs(&mut self, path: &str) -> bool {
        std::fs::create_dir_all(path).is_ok()
    }

    fn write_text(&mut self, path: &str, text: &str) -> bool {
        atomic_write(path, text).is_ok()
    }

    fn list_dir(&mut self, path: &str) -> Option<Vec<String>> {
        let rd = std::fs::read_dir(path).ok()?;
        Some(
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
        )
    }

    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }
}

/// The atomic write — ONE for the whole product, it lives in [`crate::utils`].
///
/// Here it is only re-declared under its former name: the crash report was the
/// first to get it, and both windows (`rexon-gui` and the Python bridge) call it
/// from here. The body moved to the shared place when it turned out that another
/// dozen and a half places were writing into the buyer's profile bypassing this
/// door.
pub use crate::utils::atomic_write;

// ============================================================================
// Crash dialog
// ============================================================================

/// Window title (in the interface language).
pub fn crash_dialog_title() -> String {
    // TRANSLATORS: title of the window shown after Rexon crashes.
    tr("Rexon — An Error Occurred")
}

/// Where the buyer sends the crash report. One point for BOTH windows — the Rust
/// window (`rexon-gui::crash_report`) and the Python window (`qt_main/app.py`
/// via the bridge) — so that their texts do not diverge.
///
/// `None` — there is no public tracker yet: the window names nothing and no one,
/// it only saves the report and lets it be copied. That is how it should be while
/// there is no repository: sending a person "to GitHub" where nothing exists is
/// a sure way to make them write nowhere (rule #13).
///
/// `Some("https://github.com/<owner>/<repository>/issues")` — the
/// [`crash_button_report`] button appears in the window, and the text asks to
/// report the problem on GitHub. Nothing else needs to change: labels, texts and
/// the button are assembled from here by themselves.
pub const ISSUE_TRACKER_URL: Option<&str> = None;

// The window text is assembled from two parts: "what happened" (depends on
// whether the process survived) and "what to do about it" (depends on whether a
// tracker is set). The parts are separate catalog strings joined by an empty
// line; button names in them are substituted from the labels of the buttons
// themselves, so that the text and the button do not diverge in any language.

/// "What happened": `fatal` — the process will not survive.
pub fn crash_lead(fatal: bool) -> String {
    if fatal {
        tr("Rexon quit unexpectedly.")
    } else {
        tr("An error occurred in Rexon. The program is still running, but the last \
            action may not have completed. If something stops responding, close Rexon \
            and open it again.")
    }
}

/// The "what to do" tail when the tracker address is not set (with an empty line
/// in front — it is appended to [`crash_lead`]).
pub fn crash_tail_plain() -> String {
    format!(
        "\n\n{}",
        trf(
            // TRANSLATORS: %1 is the label of the “Copy Report” button.
            "You can copy the error report with the “%1” button and attach it to your \
             message if you contact the developer.",
            &[&crash_button_copy()],
        )
    )
}

/// The "what to do" tail when the tracker address is set.
pub fn crash_tail_tracker() -> String {
    format!(
        "\n\n{}",
        trf(
            // TRANSLATORS: %1 is the label of the “Report a Problem” button, %2 of the “Copy Report” button.
            "Help fix this error: click “%1”, describe on GitHub what happened and paste \
             the report into your message — the “%2” button copies it.",
            &[&crash_button_report(), &crash_button_copy()],
        )
    )
}

/// Label of the "copy" button.
pub fn crash_button_copy() -> String {
    tr("Copy Report")
}
/// Label of the "open folder" button.
pub fn crash_button_open() -> String {
    tr("Open Folder")
}
/// Label of the "close" button.
pub fn crash_button_close() -> String {
    tr("Close")
}
/// Label of the "report a problem" button. There is no button until
/// [`ISSUE_TRACKER_URL`] is set — there is nowhere to send the person.
pub fn crash_button_report() -> String {
    tr("Report a Problem")
}

/// The tracker address fit for display: an empty string means "not set".
pub fn issue_tracker_url(url: Option<&str>) -> Option<&str> {
    url.map(str::trim).filter(|u| !u.is_empty())
}

/// The "Report a problem" button: the label and the command opening the address.
/// `None` — the address is not set, there is no button in the window.
///
/// The Rust window opens the address itself (`QDesktopServices` via
/// `Qt.openUrlExternally`), the Python window needs a command — hence the argv.
pub fn crash_report_issue_button_for(url: Option<&str>) -> Option<(String, Vec<String>)> {
    issue_tracker_url(url)
        .map(|u| (crash_button_report(), vec!["xdg-open".to_string(), u.to_string()]))
}

/// The same for the product's address ([`ISSUE_TRACKER_URL`]).
pub fn crash_report_issue_button() -> Option<(String, Vec<String>)> {
    crash_report_issue_button_for(ISSUE_TRACKER_URL)
}

/// Button role in `QMessageBox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    Action,
    Reject,
}

impl ButtonRole {
    /// Name of the `QMessageBox.ButtonRole` element for the Qt layer.
    pub fn as_str(self) -> &'static str {
        match self {
            ButtonRole::Action => "ActionRole",
            ButtonRole::Reject => "RejectRole",
        }
    }
}

/// Which button was pressed. The order matches the order of adding to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashButton {
    Copy,
    OpenFolder,
    Close,
}

/// Identifiers of the window's buttons in the order of adding. The labels are in
/// [`crash_buttons`]: the order and roles do not depend on the language, while
/// the labels do.
pub const CRASH_BUTTON_ORDER: &[(CrashButton, ButtonRole)] = &[
    (CrashButton::Copy, ButtonRole::Action),
    (CrashButton::OpenFolder, ButtonRole::Action),
    (CrashButton::Close, ButtonRole::Reject),
];

impl CrashButton {
    /// Button label in the interface language.
    pub fn label(self) -> String {
        match self {
            CrashButton::Copy => crash_button_copy(),
            CrashButton::OpenFolder => crash_button_open(),
            CrashButton::Close => crash_button_close(),
        }
    }
}

/// Window buttons in the order of adding: identifier, label, role.
pub fn crash_buttons() -> Vec<(CrashButton, String, ButtonRole)> {
    CRASH_BUTTON_ORDER.iter().map(|&(b, role)| (b, b.label(), role)).collect()
}

/// Contents of the crash window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDialog {
    pub title: String,
    pub text: String,
    /// The process will not survive: "what happened" in the text depends on this,
    /// and the system window builds its variant by this flag (not by comparing
    /// text).
    pub fatal: bool,
    /// `None` — the log could not be saved, the line is not shown.
    pub informative: Option<String>,
    /// The full report text (header + traceback) — the same text goes to the clipboard.
    pub detailed: String,
}

/// Build the crash window. `header` — the same header that went into the file.
pub fn crash_dialog(header: &str, crash_text: &str, log_path: Option<&str>) -> CrashDialog {
    CrashDialog {
        title: crash_dialog_title(),
        text: crash_dialog_text(true),
        fatal: true,
        informative: log_path.map(msg_report_saved),
        detailed: format!("{header}{crash_text}"),
    }
}

/// "Report saved" with the path on the next line.
fn msg_report_saved(path: &str) -> String {
    // TRANSLATORS: %1 is the path of the crash report file (shown on its own line).
    trf("Report saved:\n%1", &[&path])
}

/// What to do on the pressed button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashAction {
    /// Put the text into the clipboard.
    Copy(String),
    /// Run a command (opening the folder with the log).
    Run(Vec<String>),
    /// Nothing.
    None,
}

/// Reaction to a button. "Open folder" without a saved log does nothing: in the
/// original the branch requires both the button and the path.
pub fn crash_button_action(
    dialog: &CrashDialog,
    button: CrashButton,
    log_path: Option<&str>,
) -> CrashAction {
    match button {
        CrashButton::Copy => CrashAction::Copy(dialog.detailed.clone()),
        CrashButton::OpenFolder => match log_path {
            Some(p) => CrashAction::Run(crash_open_folder_argv(p)),
            None => CrashAction::None,
        },
        CrashButton::Close => CrashAction::None,
    }
}

/// argv for opening the log directory in the file manager.
pub fn crash_open_folder_argv(log_path: &str) -> Vec<String> {
    vec!["xdg-open".to_string(), dirname(log_path)]
}

/// Port of `os.path.dirname`: the part before the last `/`; at the root the slash
/// remains, for a name without a directory — an empty string.
fn dirname(path: &str) -> String {
    match path.rfind('/') {
        None => String::new(),
        Some(i) => {
            let head = &path[..=i];
            let trimmed = head.trim_end_matches('/');
            if trimmed.is_empty() {
                "/".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

// ============================================================================
// The exception hook's decision
// ============================================================================

/// Exceptions the buyer must NOT be told about: they are regular ways of
/// quitting, not a crash.
///
/// `SystemExit` is added here — fix No. 2, see the module header.
pub const SILENT_EXCEPTIONS: &[&str] = &["KeyboardInterrupt", "SystemExit"];

/// What to do with a caught exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashDecision {
    /// Hand the traceback to the regular `sys.__excepthook__` (for the terminal).
    pub print_stderr: bool,
    /// Write a report into the report directory.
    pub save_log: bool,
    /// Show the crash window.
    pub show_dialog: bool,
    /// The showing must be handed over to the GUI thread: the exception was caught in
    /// a worker thread, and building a `QMessageBox` in place is not allowed (fix
    /// No. 3).
    pub dialog_needs_main_thread_hop: bool,
}

/// Decision by the exception class name and the thread where it was caught.
///
/// In the main thread a "silent" one is still printed to stderr (the original
/// does so: `Ctrl+C` from the terminal must leave a trace). In a worker thread a
/// "silent" one does nothing at all.
pub fn crash_decision(exc_type_name: &str, main_thread: bool) -> CrashDecision {
    let silent = SILENT_EXCEPTIONS.contains(&exc_type_name);
    if silent {
        return CrashDecision {
            print_stderr: main_thread,
            save_log: false,
            show_dialog: false,
            dialog_needs_main_thread_hop: false,
        };
    }
    CrashDecision {
        print_stderr: true,
        save_log: true,
        show_dialog: true,
        dialog_needs_main_thread_hop: !main_thread,
    }
}

// ============================================================================
// Panic of the Rust window
// ============================================================================
//
// The Rust entry point (`rexon-main-window`) has panics instead of exceptions,
// and the panic hook is called for EACH of them, including ones caught higher
// up the stack (`catch_unwind`, a tokio task). Hence two differences from the
// decision for Python:
//   * the report and the window — once per process: a second panic (most often
//     a consequence of the first) goes only to stderr, otherwise the person
//     would get a pile of windows in a row;
//   * a panic in the main thread is always fatal (leaving `main` or an `abort`
//     at the Qt boundary), while in a worker thread the process as a rule lives
//     on — and the window must tell the truth: "keeps running", not "quit
//     unexpectedly". The original said "quit" even where only a worker thread
//     died (a fix per rule #16).

/// Labels for expanding the technical text. The details are collapsed by
/// default: a traceback tells the buyer nothing (rule #13), it is needed by the
/// person they will send the report to.
pub fn crash_details_show() -> String {
    tr("Show Details")
}
pub fn crash_details_hide() -> String {
    tr("Hide Details")
}

/// The line after pressing "Copy report". The window does NOT close at this
/// point: on Wayland and X11 the clipboard lives as long as its owner, and
/// closing the window right after copying would take the copied text away with
/// it (if the session has no clipboard manager).
pub fn crash_copied() -> String {
    tr("Report copied — paste it into your message to the developer.")
}

/// The window's main text by whether the process died and whether the tracker
/// address is set.
pub fn crash_dialog_text_for(fatal: bool, url: Option<&str>) -> String {
    let tail = if issue_tracker_url(url).is_some() {
        crash_tail_tracker()
    } else {
        crash_tail_plain()
    };
    format!("{}{tail}", crash_lead(fatal))
}

/// The same for the product's address ([`ISSUE_TRACKER_URL`]).
pub fn crash_dialog_text(fatal: bool) -> String {
    crash_dialog_text_for(fatal, ISSUE_TRACKER_URL)
}

/// What to do with a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanicDecision {
    /// Write a report into the report directory.
    pub save_log: bool,
    /// Show the crash window (as a separate process).
    pub show_dialog: bool,
    /// The process will not survive this panic — the window text depends on this.
    pub fatal: bool,
}

/// Decision by the panic's thread and by whether there has already been a report
/// in this process. The panic is ALWAYS printed to stderr — this is not decided.
pub fn panic_decision(main_thread: bool, already_reported: bool) -> PanicDecision {
    PanicDecision {
        save_log: !already_reported,
        show_dialog: !already_reported,
        fatal: main_thread,
    }
}

/// Body of a panic report — what the traceback takes for the Python host.
pub fn panic_crash_text(message: &str, thread: &str, location: Option<&str>, backtrace: &str) -> String {
    let location = location.unwrap_or("?");
    let backtrace = backtrace.trim_end();
    format!(
        "Rust panic: {message}\n\
         Thread:   {thread}\n\
         Location: {location}\n\n\
         Backtrace:\n{backtrace}\n"
    )
}

/// The crash window from a saved report file (the window process reads it).
///
/// "Report saved" is shown only if the file lies right in the report directory:
/// if saving there failed, the report travels to the window via a temporary file
/// ([`crash_transport_path`]), and naming it to the person as saved would be
/// untrue — it will disappear when the session ends.
pub fn crash_dialog_for_report(report: &str, report_path: &str, crash_dir: &str, fatal: bool) -> CrashDialog {
    let saved = dirname(report_path) == crash_dir.trim_end_matches('/');
    CrashDialog {
        title: crash_dialog_title(),
        text: crash_dialog_text(fatal),
        fatal,
        informative: saved.then(|| msg_report_saved(report_path)),
        detailed: report.to_string(),
    }
}

/// Where to put the report for the window if writing into the report directory
/// failed. `runtime_dir` — `$XDG_RUNTIME_DIR` (or a temporary directory).
pub fn crash_transport_path(runtime_dir: &str, pid: u32) -> String {
    format!("{}/rexon-crash-{pid}.log", runtime_dir.trim_end_matches('/'))
}

/// Whether there is anywhere to show the crash window. Without a display (session
/// logout, launch from a console) any window — ours and kdialog — dies with
/// `abort`: that means a core dump and someone else's crash window instead of
/// ours, and there is nothing to show anyway. Then the report stays a file and a
/// line in stderr.
pub fn crash_dialog_can_show(wayland_display: Option<&str>, display: Option<&str>) -> bool {
    [wayland_display, display]
        .iter()
        .any(|v| v.is_some_and(|s| !s.is_empty()))
}

/// Whether Qt will come up in this process at all. There is a display
/// ([`crash_dialog_can_show`]) — yes. There is none — yes only when
/// `QT_QPA_PLATFORM` explicitly names a platform without a screen (`offscreen`,
/// `minimal`, `eglfs`, `linuxfb`, `vnc`…): with `wayland*` and `xcb` without a
/// display `QGuiApplication` ends in `abort`. A list of platforms separated by
/// `;` (fallbacks) and parameters after `:` are understood the same way as by Qt.
///
/// One answer for the same class as the crash window: "Qt without a display dies
/// with abort" — any entry point that creates a `QGuiApplication` and may be
/// started from a console must ask it.
pub fn qt_can_start(wayland_display: Option<&str>, display: Option<&str>, qpa_platform: Option<&str>) -> bool {
    if crash_dialog_can_show(wayland_display, display) {
        return true;
    }
    qpa_platform.unwrap_or_default().split(';').any(|p| {
        let name = p.split(':').next().unwrap_or_default().trim();
        !name.is_empty() && name != "xcb" && !name.starts_with("wayland")
    })
}

/// The "what to do" tail for the system window without a tracker address: our
/// buttons are not there, so instead of "copy it with the button" — "attach the
/// file".
pub fn crash_fallback_file_hint() -> String {
    format!(
        "\n\n{}",
        tr("If you contact the developer, attach the report file to your message — its \
            path is below.")
    )
}

/// The "what to do" tail for the system window: there are no buttons, so the
/// tracker address is named right in the text, and the report is offered to be
/// attached as a file (if it was saved — otherwise there is nothing to attach,
/// and we keep quiet about the file).
fn crash_fallback_tail(url: Option<&str>, saved: bool) -> String {
    match (issue_tracker_url(url), saved) {
        (None, true) => crash_fallback_file_hint(),
        (None, false) => String::new(),
        (Some(u), true) => format!(
            "\n\n{}",
            trf(
                // TRANSLATORS: %1 is the web address of the issue tracker.
                "Help fix this error: report it on GitHub (%1) and attach the report file \
                 to your message — its path is below.",
                &[&u],
            )
        ),
        (Some(u), false) => format!(
            "\n\n{}",
            // TRANSLATORS: %1 is the web address of the issue tracker.
            trf("Help fix this error: report it on GitHub — %1", &[&u])
        ),
    }
}

/// System windows for the case when the Rexon window did not come up (Qt does
/// not start, the binary does not launch). The order is the order of attempts;
/// each window reports the same as the Rexon window, except for the buttons.
///
/// kdialog first: it can show "details" under an expander; zenity has only the
/// main text (a traceback is unreadable in it), but both have the path to the
/// report file.
pub fn crash_fallback_argvs(dialog: &CrashDialog) -> Vec<Vec<String>> {
    crash_fallback_argvs_for(dialog, ISSUE_TRACKER_URL)
}

/// The same with an explicit tracker address (the product one is
/// [`ISSUE_TRACKER_URL`]).
pub fn crash_fallback_argvs_for(dialog: &CrashDialog, url: Option<&str>) -> Vec<Vec<String>> {
    // System windows do not have our buttons, so the "what to do" tail is
    // replaced entirely: advice about buttons is replaced by the report file
    // and the tracker address. "What happened" is taken by the flag, not by
    // cutting the tail off the text: a text comparison would depend on the
    // language.
    let lead = crash_lead(dialog.fatal);
    let mut body = format!(
        "{lead}{}",
        crash_fallback_tail(url, dialog.informative.is_some())
    );
    if let Some(info) = &dialog.informative {
        body.push_str("\n\n");
        body.push_str(info);
    }
    let s = |v: &str| v.to_string();
    vec![
        vec![
            s("kdialog"),
            s("--title"),
            dialog.title.clone(),
            s("--detailederror"),
            body.clone(),
            dialog.detailed.clone(),
        ],
        vec![
            s("zenity"),
            s("--error"),
            s("--no-markup"),
            s("--width=520"),
            s("--title"),
            dialog.title.clone(),
            s("--text"),
            body,
        ],
    ]
}

// ============================================================================
// Diagnostic start-up lines
// ============================================================================

/// The line "which Qt platform was chosen" — printed right after creating the
/// `QApplication`. The original's `!r`: strings in quotes, a missing environment
/// variable — `None`.
pub fn qt_platform_banner(platform_name: &str, qt_qpa_platform: Option<&str>) -> String {
    let qpa = match qt_qpa_platform {
        Some(v) => py_repr_str(v),
        None => "None".to_string(),
    };
    format!(
        "[QT] platform={}, QT_QPA_PLATFORM={qpa}",
        py_repr_str(platform_name)
    )
}

/// The line "with what flags and geometry the window came up" — printed after
/// `show()`. The geometry is printed as a Python tuple.
pub fn qt_window_banner(flags: u64, frame_geometry: (i32, i32, i32, i32)) -> String {
    let (x, y, w, h) = frame_geometry;
    format!("[QT] flags=0x{flags:x}, frameGeom=({x}, {y}, {w}, {h})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header of a real Python host report (`crash_20260907_220608.log` from
    /// this machine) — the layout reference for both entry points.
    const PY_SAMPLE_HEADER: &str = "Rexon crash report\n\
        Time:     2026-09-07T22:06:08.454403\n\
        Python:   3.14.7\n\
        PySide6:  6.11.2\n\
        OS:       Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44\n\
        DE:       KDE (wayland)\n\
        Locale:   ru_RU.UTF-8\n\
        ────────────────────────────────────────────────────────────\n\n";

    fn sample_now() -> Timestamp {
        Timestamp { year: 2026, month: 9, day: 7, hour: 22, minute: 6, second: 8, microsecond: 454403 }
    }

    fn sample_env(os: &str) -> CrashHeaderEnv<'_> {
        CrashHeaderEnv {
            os_platform: os,
            xdg_current_desktop: Some("KDE"),
            desktop_session: Some("plasma"),
            xdg_session_type: Some("wayland"),
            lang: Some("ru_RU.UTF-8"),
        }
    }

    /// Directory for writing — in $HOME, not in /tmp (that is RAM, and other
    /// people's files live there). Unique per run.
    fn home_tmp(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join(".cache")
            .join(format!("rexon-test-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn python_header_still_matches_real_sample() {
        let os = os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44")));
        let e = sample_env(&os);
        let got = crash_header(&CrashHeaderInput {
            now: sample_now(),
            python_version: "3.14.7",
            pyside_version: Some("6.11.2"),
            os_platform: e.os_platform,
            xdg_current_desktop: e.xdg_current_desktop,
            desktop_session: e.desktop_session,
            xdg_session_type: e.xdg_session_type,
            lang: e.lang,
        });
        assert_eq!(got, PY_SAMPLE_HEADER);
    }

    #[test]
    fn rust_header_has_python_layout() {
        let os = os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44")));
        let got = rust_crash_header(&RustCrashHeaderInput {
            now: sample_now(),
            rexon_version: "0.0.1",
            entry: "rexon-main-window",
            qt_version: Some("6.11.2"),
            env: sample_env(&os),
        });
        let expected = PY_SAMPLE_HEADER
            .replace("Python:   3.14.7\n", "Rexon:    0.0.1 (rexon-main-window)\n")
            .replace("PySide6:  6.11.2\n", "Qt:       6.11.2\n");
        assert_eq!(got, expected);
    }

    #[test]
    fn rust_header_unknowns() {
        let got = rust_crash_header(&RustCrashHeaderInput::default());
        assert!(got.contains("\nQt:       ?\n"), "{got}");
        assert!(got.contains("\nDE:       ? (?)\n"), "{got}");
        assert!(got.contains("\nRexon:    \n"), "{got}");
    }

    #[test]
    fn os_platform_like_python() {
        assert_eq!(
            os_platform("Linux", "7.2.3-1-cachyos", "x86_64", Some(("glibc", "2.44"))),
            "Linux-7.2.3-1-cachyos-x86_64-with-glibc2.44"
        );
        assert_eq!(os_platform("Linux", "6.1", "aarch64", None), "Linux-6.1-aarch64");
        assert_eq!(os_platform("Linux", "", "x86_64", None), "Linux-x86_64");
    }

    #[test]
    fn panic_reported_once_and_fatal_only_on_main() {
        assert_eq!(
            panic_decision(true, false),
            PanicDecision { save_log: true, show_dialog: true, fatal: true }
        );
        let worker = panic_decision(false, false);
        assert!(worker.save_log && worker.show_dialog && !worker.fatal);
        let again = panic_decision(true, true);
        assert!(!again.save_log && !again.show_dialog);
    }

    #[test]
    fn panic_text_layout() {
        let t = panic_crash_text("boom", "main", Some("src/x.rs:1:2"), "   0: f\n   1: g\n\n");
        assert_eq!(
            t,
            "Rust panic: boom\nThread:   main\nLocation: src/x.rs:1:2\n\nBacktrace:\n   0: f\n   1: g\n"
        );
        assert!(panic_crash_text("x", "w", None, "").contains("Location: ?\n"));
    }

    #[test]
    fn dialog_for_report_names_only_saved_file() {
        // The source language: the Russian text is guarded by `tests/qt_app_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = "/home/u/.config/rexon/crashes";
        let saved = crash_dialog_for_report("R", &format!("{dir}/crash_1.log"), dir, true);
        assert_eq!(saved.title, crash_dialog_title());
        assert_eq!(saved.text, crash_dialog_text(true));
        assert!(saved.fatal);
        assert_eq!(saved.informative.as_deref(), Some("Report saved:\n/home/u/.config/rexon/crashes/crash_1.log"));
        assert_eq!(saved.detailed, "R");
        // The same look as the Python host's window for the same header+text pair.
        let py = crash_dialog("", "R", Some(&format!("{dir}/crash_1.log")));
        assert_eq!(saved, py);

        let moved = crash_dialog_for_report("R", &crash_transport_path("/run/user/1000/", 42), dir, false);
        assert_eq!(moved.informative, None);
        assert_eq!(moved.text, crash_dialog_text(false));
        assert!(!moved.fatal);
        assert_eq!(crash_transport_path("/run/user/1000/", 42), "/run/user/1000/rexon-crash-42.log");
    }

    #[test]
    fn dialog_needs_some_display() {
        assert!(crash_dialog_can_show(Some("wayland-0"), None));
        assert!(crash_dialog_can_show(None, Some(":0")));
        assert!(!crash_dialog_can_show(None, None));
        assert!(!crash_dialog_can_show(Some(""), Some("")));
    }

    #[test]
    fn qt_without_display_only_on_screenless_platform() {
        assert!(qt_can_start(Some("wayland-0"), None, None));
        assert!(qt_can_start(None, Some(":0"), Some("xcb")));
        assert!(!qt_can_start(None, None, None));
        assert!(!qt_can_start(Some(""), Some(""), Some("")));
        assert!(!qt_can_start(None, None, Some("wayland")));
        assert!(!qt_can_start(None, None, Some("wayland-egl;xcb")));
        assert!(qt_can_start(None, None, Some("offscreen")));
        assert!(qt_can_start(None, None, Some("wayland;offscreen")));
        assert!(qt_can_start(None, None, Some("vnc:size=800x600")));
    }

    #[test]
    fn fallback_dialogs_carry_path_and_title() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let d = crash_dialog_for_report("DETAILS", "/c/crash_1.log", "/c", true);
        let argvs = crash_fallback_argvs(&d);
        assert_eq!(argvs[0][0], "kdialog");
        assert_eq!(argvs[0][3], "--detailederror");
        assert!(argvs[0][4].ends_with("Report saved:\n/c/crash_1.log"));
        assert_eq!(argvs[0][5], "DETAILS");
        assert_eq!(argvs[1][0], "zenity");
        assert!(argvs[1].iter().any(|a| a == "--no-markup"));
        assert!(argvs[1].last().unwrap().ends_with("/c/crash_1.log"));
        // There is no advice about a non-existent button in the system window.
        for fatal in [true, false] {
            let tail = crash_tail_plain();
            assert!(
                crash_dialog_text(fatal).ends_with(&tail),
                "хвост текста не найден — подмена в системном окне молча отключилась"
            );
            let hint = crash_fallback_file_hint();
            let saved = crash_fallback_argvs(&crash_dialog_for_report("D", "/c/crash_1.log", "/c", fatal));
            assert!(!saved[0][4].contains(&tail) && saved[0][4].contains(&hint));
            assert!(saved[0][4].starts_with(&crash_lead(fatal)), "{}", saved[0][4]);
            let unsaved = crash_fallback_argvs(&crash_dialog_for_report("D", "/run/rexon-crash-1.log", "/c", fatal));
            assert!(!unsaved[1][7].contains(&tail) && !unsaved[1][7].contains(&hint));
        }
        for a in &argvs {
            assert!(a.iter().any(|x| *x == crash_dialog_title()));
        }
    }

    /// The tracker address decides everything: while there is none, the window names
    /// no one and does not show the "Report a problem" button; as soon as it is set,
    /// both the button and the mention of GitHub appear. Both windows, Rust and
    /// Python, take this from here, so the fork itself is checked.
    #[test]
    fn issue_tracker_switches_text_and_button() {
        const URL: &str = "https://github.com/wehrwolfmann/rexon/issues";
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (copy, report) = (crash_button_copy(), crash_button_report());

        // Empty — the state of the product today.
        assert_eq!(crash_report_issue_button_for(None), None);
        assert_eq!(crash_report_issue_button_for(Some("   ")), None);
        for fatal in [true, false] {
            let text = crash_dialog_text_for(fatal, None);
            assert!(
                !text.contains("GitHub") && !text.contains("Discord"),
                "адресата нет — называть его нечем: {text}"
            );
            assert!(text.contains(&copy));
            assert!(!text.contains(&report));
        }

        // Set — the button and a text with GitHub.
        let (label, argv) = crash_report_issue_button_for(Some(URL)).unwrap();
        assert_eq!(label, report);
        assert_eq!(argv, vec!["xdg-open".to_string(), URL.to_string()]);
        for fatal in [true, false] {
            let text = crash_dialog_text_for(fatal, Some(URL));
            assert!(text.contains("GitHub"), "текст не зовёт на трекер: {text}");
            assert!(text.contains(&report) && text.contains(&copy));
        }

        // The button labels are named in the text — we check that they have not drifted
        // from the buttons themselves.
        assert!(crash_tail_plain().contains(&copy));
        assert!(crash_tail_tracker().contains(&report));
        assert!(crash_tail_tracker().contains(&copy));

        // The system window has no buttons: the address goes right into the text.
        let d = crash_dialog_for_report("D", "/c/crash_1.log", "/c", true);
        let with = crash_fallback_argvs_for(&d, Some(URL));
        assert!(with[0][4].contains(URL) && !with[0][4].contains(&copy));
        let without = crash_fallback_argvs_for(&d, None);
        assert!(!without[0][4].contains("GitHub") && without[0][4].contains(&crash_fallback_file_hint()));
        let moved = crash_dialog_for_report("D", "/run/rexon-crash-1.log", "/c", false);
        assert!(crash_fallback_argvs_for(&moved, Some(URL))[1][7].contains(URL));

        // The product wrappers take exactly this constant.
        assert_eq!(crash_dialog_text(true), crash_dialog_text_for(true, ISSUE_TRACKER_URL));
        assert_eq!(crash_report_issue_button(), crash_report_issue_button_for(ISSUE_TRACKER_URL));
    }

    fn part_files(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".part-"))
            .collect()
    }

    #[test]
    fn atomic_write_replaces_whole_file_and_leaves_no_part() {
        let dir = home_tmp("atomic");
        let path = dir.join("crash_x.log");
        std::fs::write(&path, "старый длинный-длинный текст, который обязан исчезнуть целиком").unwrap();
        atomic_write(path.to_str().unwrap(), "новый").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "новый");
        assert!(part_files(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_failure_keeps_target_and_cleans_part() {
        let dir = home_tmp("atomic-fail");
        // A non-empty directory in place of the file: rename must refuse.
        let target = dir.join("crash_y.log");
        std::fs::create_dir_all(target.join("inner")).unwrap();
        assert!(atomic_write(target.to_str().unwrap(), "x").is_err());
        assert!(target.join("inner").is_dir());
        assert!(part_files(&dir).is_empty());
        // There is no directory at all — an error, and nothing is created.
        let missing = dir.join("nope/crash_z.log");
        assert!(atomic_write(missing.to_str().unwrap(), "x").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn live_save_is_atomic_and_rotates() {
        let root = home_tmp("save");
        let dir = root.join("rexon/crashes");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..22 {
            std::fs::write(dir.join(format!("crash_2000010{}_0000{:02}.log", i / 10, i)), "old").unwrap();
        }
        std::fs::write(dir.join("README"), "чужой").unwrap();
        let ts = Timestamp { year: 2026, month: 9, day: 11, hour: 12, minute: 0, second: 1, microsecond: 0 };
        let path = save_crash_log(&mut AtomicCrashIo, dir.to_str().unwrap(), &ts, "H\n", "T\n").unwrap();
        assert!(path.ends_with("/crash_20260911_120001.log"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "H\nT\n");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.iter().filter(|n| n.starts_with(CRASH_PREFIX)).count(), CRASH_KEEP);
        assert!(names.contains(&"README".to_string()));
        assert!(part_files(&dir).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
