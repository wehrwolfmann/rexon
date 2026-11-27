//! The Rexon install executor — into the system, into the home folder and
//! portable mode. Port of `RexonInstaller.install` / `install_to_system` /
//! `install_user` / `_install_portable` (`rexon/installer.py`) on top of core
//! data ([`crate::installer`], [`crate::system_plan`]). The entry point for the
//! window is the dispatcher [`install`].
//!
//! The split is the same as in the neighbours [`crate::system_exec`] and
//! [`crate::system_de_exec`]: **the core decides, the executor does**.
//!   * WHAT to install and WHERE — the [`InstallLayout`] layout, the root
//!     script [`install_script`] and the plan [`install_plan`];
//!   * WITH WHAT — the [`InstallIo`] trait (an extension of [`SystemIo`]): in
//!     the product [`LiveInstallIo`], in tests — a substitute, the live machine
//!     is not touched;
//!   * progress goes to [`DeOpSink`], with the same contract as desktop
//!     operations: the `DeOperationDialog` progress window plugs in without
//!     changes (the bridge is item G5). All strings for the person come from
//!     the "Wizard texts" section of [`crate::installer`] (`InstallStep`,
//!     `InstallFailure`, `WizardOutcome`); there are no letters of our own here.
//!
//! ⭐ WHAT WAS FIXED IN THE PORT (rule #16), each proven by a test below:
//!   1. Any root script failure was reported as "Installation cancelled by the
//!      user (pkexec)": the whole `CalledProcessError` was caught, and a
//!      failed dependency install read as a cancel. Now the script prints step
//!      markers, and the outcome is told apart by FACT ([`classify_script_run`]):
//!      no markers and code 126/127 (or an agent refusal) — the password was
//!      not confirmed, the script didn't run; markers present — the named step
//!      broke, even if code 127 came from a command inside.
//!   2. Temporary files and the package copy stayed in `/tmp` on error and
//!      cancel (cleanup was only on the success path; the owner's `/tmp` is
//!      RAM). Now everything temporary lives in ONE directory, and it is
//!      removed on any outcome after it is created.
//!   3. Root script timeout 300 s → 600 s
//!      ([`inst::SYSTEM_INSTALL_TIMEOUT_SECS`]).
//!   4. `chmod -R 755` set the execute bit on ALL package files, and
//!      `cp -r pkg/*` lost hidden files. Now permissions are laid out during
//!      preparation ([`payload_mode`]: directories 755, files 644, executables
//!      755), the script copies `dir/.` preserving permissions and hands
//!      ownership to root.
//!   5. Manual install installed ONLY the Python package and the Python
//!      launcher. Now the content is the same as the distribution package's:
//!      `rexon-main-window`, `rexon-wallpaper-host`, `rexon-splash-overlay`,
//!      `librexon_qt_filter.so`, the QML module directory and product data
//!      (vendored QML of the wallpaper tab, `themes.json`, fonts, the Plasma
//!      plugin and the GNOME and Cinnamon add-ons). There are no Python sources
//!      at all ([`payload_skip`]), the `rexon` command is an `sh` launcher for
//!      the window binary ([`inst::system_launcher`]), and a payload without
//!      this binary is rejected before the first side effect ([`payload_has_window`]).
//!   6. The installer's layout disagreed with the lookup: the package was put
//!      straight into `LIB_DIR`, while the Rust window looked for
//!      `<LIB_DIR>/rexon`; the QML module directory and the window binary in
//!      `/usr/local` weren't looked up at all. The layout is now one — the same
//!      as the distribution package's ([`InstallLayout`]); the guard
//!      `layout_is_one_for_package_and_manual_install` checks it against the lookup.
//!   7. ⭐⭐ The `rules.d` rule silently gave the wheel group all `org.rexon.*`
//!      WITHOUT a password, including installing and removing ANY packages.
//!      Now it is a separate step only with explicit consent
//!      ([`InstallRequest::passwordless_rule`], default
//!      [`inst::PASSWORDLESS_CONSENT_DEFAULT`]), narrowed to
//!      [`plan::PASSWORDLESS_ACTIONS`] and aware of the family's admin group
//!      ([`plan::polkit_admin_groups`]).
//!
//! ⭐ WHAT WAS FIXED FOR THE HOME FOLDER AND PORTABLE MODE (G3b), also with tests:
//!   8. Installing into the home folder didn't create the backups folder
//!      (system and portable did), and portable mode with an empty
//!      `XDG_DATA_HOME` created `rexon/backups` in the CURRENT directory.
//!      Profile folders are now the same for all modes ([`profile_dirs`]).
//!   9. The previous copy in `~/.local/lib/rexon` was deleted BEFORE copying
//!      the new one: a failure in the middle of the copy left the person with
//!      neither. Now the new copy is assembled next to it, and the copies are
//!      swapped by renaming; on failure the previous one stays in place.
//!  10. On an immutable system "into the home folder" is chosen BEFORE the
//!      system install ([`install`]), not by a refusal inside it: the summary
//!      phrase "some changes may have remained" would be untrue.
//!  11. The lookup didn't know the home folder install layout: the window
//!      binary (`installer::main_window_candidates`), QML modules
//!      (`qml_modules::module_dirs`) and the package
//!      (`installer::package_data_dir_candidates`).
//!  12. The launcher, desktop entries and icons were written OVER the existing
//!      path: if there was a link (for the developer — into the tree), the
//!      write would go into its target. Now a link is replaced, and an install
//!      directory that is itself a link is not replaced at all
//!      ([`is_redirected`]). A path with an apostrophe broke the Python
//!      launcher; the pixmaps icon was taken only from 128 px.
//!  13. A running wallpaper host of the previous install can't be opened for
//!      overwriting (`ETXTBSY`): binaries are installed via a temporary name
//!      and a rename.
//!  14. Menu and icon caches — the same commands for both modes
//!      ([`cache_refresh_groups`]); a missing program is not an error, and
//!      without `update-desktop-database` the person learns that the desktop
//!      entry will appear later.
//!
//! ⭐ UNINSTALL (G8) — the same executor, entry point [`uninstall`]; the window's
//! «Uninstall Rexon» menu no longer has an executor of its own. Fixed, with tests:
//!  15. The wallpaper host and the splash in the command directory remained
//!      after uninstalling in both modes — the paths now come from
//!      [`InstallLayout::extra_bin_paths`].
//!  16. A link in place of the Rexon folder (for the developer
//!      `/usr/local/lib/rexon` points into the working tree) was wiped by the
//!      root script, and so was a link to the settings folder. Now a path that
//!      is a link is neither deleted nor traversed in any mode
//!      ([`is_redirected`]); the list in the question dialog doesn't name it,
//!      and the summary says it was left.
//!  17. Any failure read as one line "Uninstall error system: code …". A
//!      password refusal, no way to ask for a password, a timeout and a failure
//!      of the removal itself are different outcomes ([`classify_script_run`]
//!      by the script start marker), and the words carry no jargon
//!      ([`inst::uninstall_failure_line`]).
//!  18. The temporary uninstall script lay right in `/tmp` and stayed there on
//!      failure; now it is in a temporary folder removed on any outcome.
//!  19. When Rexon wasn't installed at all, uninstall still wiped
//!      `~/.config/rexon` and answered "Rexon is not installed". Now nothing is
//!      touched.
//!  20. (Wehrwolfmann's decision of 15.09.2026) «Keep» kept only the desktop
//!      backups and wiped the settings — after reinstalling the person started
//!      from scratch. Now the [`UninstallRequest::keep_profile`] option touches
//!      neither the settings (with the installed themes journal) nor the
//!      backups; «Remove Everything» — as before, both folders.
//!  21. (Wehrwolfmann's decision of 15.09.2026) No choice removed the cache
//!      (`$XDG_CACHE_HOME/rexon`) and the installed wallpapers, themes and
//!      add-ons (`$XDG_DATA_HOME/rexon/*`), nor the literal `~/.cache/rexon` and
//!      `~/.local/share/rexon` that part of the core writes to bypassing XDG.
//!      Now the cache goes with any choice, the data content — only with
//!      «Remove Everything»; the list is one — [`inst::user_data_roots`]. A
//!      link in place of the cache or content is removed itself, without
//!      following it.
//!
//! ⛔ The live [`install`] with [`LiveInstallIo`] is never called in tests:
//! guarded by `tests_never_build_live_io`. Home folder tests touch the real FS
//! only inside their own temporary directory — every path is checked against
//! it, and leaving it fails the test.

use std::sync::Arc;

use crate::de_progress;
use crate::distro_compat::Lang;
use crate::i18n::{tr, trf};
use crate::installer::{
    self as inst, HelperTmp, IconSet, InstallFailure, InstallMode, InstallStep, LaunchEntry,
    WizardOutcome,
};
use crate::qml_modules as qm;
use crate::system::PkgFamily;
use crate::system_de_exec::{CancelToken, DeOpSink, DeOpsIo, LiveDeOpsIo};
use crate::system_exec::{RunOut, SystemIo};
use crate::system_plan::{self as plan, Cmd, PlanStep, RunStepId};
use crate::utils::{self, RunOutcome};

// ============================================================================
// Layout: one for the package, the manual install and the archive
// ============================================================================

/// Product data directory `rexon/` inside the Rexon data directory: vendored
/// QML of the wallpaper tab, `themes.json`, fonts, the Plasma wallpaper plugin
/// and the GNOME and Cinnamon add-ons. There is no Python code there any more ([`payload_skip`]).
pub const PACKAGE_DIR_NAME: &str = "rexon";
/// QML module directory inside the Rexon data directory.
pub const QML_MODULES_DIR_NAME: &str = "qml_modules";
/// Executables directory inside the unpacked archive.
pub const BUNDLE_BIN_DIR: &str = "bin";
/// Executables directory of the distribution package (PKGBUILD puts the
/// wallpaper host and the splash there).
pub const PACKAGE_BIN_DIR: &str = "/usr/bin";

/// Where things lie in an installed Rexon. The layout is ONE for all channels:
///
/// | channel | `lib_dir` | `bin_dir` |
/// |---|---|---|
/// | distribution package | `/usr/lib/rexon` | `/usr/bin` |
/// | system install | `/usr/local/lib/rexon` | `/usr/local/bin` |
/// | home folder install | `~/.local/lib/rexon` | `~/.local/bin` |
/// | unpacked archive | `<root>` | `<root>/bin` |
///
/// Inside `lib_dir`: product data `rexon/`, the window binary, the QML module
/// directory with our `.so` in `waywallen/control`. In `bin_dir`: the `rexon`
/// command, the wallpaper host and the splash (the core looks them up via `PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallLayout {
    pub lib_dir: String,
    pub bin_dir: String,
}

impl InstallLayout {
    pub fn new(lib_dir: &str, bin_dir: &str) -> Self {
        InstallLayout {
            lib_dir: lib_dir.trim_end_matches('/').to_string(),
            bin_dir: bin_dir.trim_end_matches('/').to_string(),
        }
    }

    /// Layout of an install mode (`None` for portable — there is nowhere to install).
    pub fn for_mode(mode: InstallMode, home: &str, xdg_data_home: Option<&str>) -> Option<Self> {
        let p = inst::install_paths(mode, home, xdg_data_home);
        Some(Self::new(p.lib_dir.as_deref()?, p.install_dir.as_deref()?))
    }

    /// System install.
    pub fn system() -> Self {
        Self::new(inst::LIB_DIR, inst::INSTALL_DIR)
    }

    /// Distribution package.
    pub fn package() -> Self {
        Self::new(inst::package_data_dir(), PACKAGE_BIN_DIR)
    }

    /// Unpacked archive with root `root`.
    pub fn bundle(root: &str) -> Self {
        let root = root.trim_end_matches('/');
        Self::new(root, &format!("{root}/{BUNDLE_BIN_DIR}"))
    }

    pub fn package_dir(&self) -> String {
        format!("{}/{PACKAGE_DIR_NAME}", self.lib_dir)
    }

    pub fn main_window(&self) -> String {
        format!("{}/{}", self.lib_dir, crate::window_menu::MAIN_WINDOW_BIN)
    }

    /// The «Appearance» tiles helper — next to the window: that is where the core
    /// looks for it (`widget_shot::find_helper`).
    pub fn theme_shot(&self) -> String {
        format!("{}/{}", self.lib_dir, crate::widget_shot::HELPER_BINARY)
    }

    pub fn qml_modules(&self) -> String {
        format!("{}/{QML_MODULES_DIR_NAME}", self.lib_dir)
    }

    /// Directory of our `.so` next to the `waywallen.control` plugin.
    pub fn rust_lib_dir(&self) -> String {
        format!("{}/{}", self.qml_modules(), qm::RUST_LIB_SUBDIR)
    }

    pub fn filter_lib(&self) -> String {
        format!("{}/{}", self.rust_lib_dir(), qm::FILTER_LIB_NAME)
    }

    pub fn wallpaper_host(&self) -> String {
        format!("{}/{}", self.bin_dir, crate::wallpaper_engine::WALLPAPER_HOST_BINARY)
    }

    pub fn splash_overlay(&self) -> String {
        format!("{}/{}", self.bin_dir, crate::session_splash::SPLASH_OVERLAY_BINARY)
    }

    /// The product command `rexon` — a launcher for the window binary
    /// ([`inst::system_launcher`]).
    pub fn launcher(&self) -> String {
        format!("{}/{}", self.bin_dir, inst::PYTHON_ENTRY_CMD)
    }

    /// Install files in `bin_dir` beyond the launcher. Uninstall in both modes
    /// takes them from here too (`installer::uninstall_system_paths`,
    /// `installer::uninstall_user_paths`) — one source of truth.
    pub fn extra_bin_paths(&self) -> Vec<String> {
        vec![self.wallpaper_host(), self.splash_overlay()]
    }
}

// ============================================================================
// What we install: the payload source
// ============================================================================

/// Sizes of the Rexon raster icons (`rexon_icon_<N>.png`).
pub const ICON_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

/// Where to take files from. `None` — this part is not in the payload (for
/// example, a copy without the Rust window: then the desktop entry leads to the classic window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadSource {
    /// The `rexon/` package.
    pub package_dir: String,
    /// Directory with the `rexon_icon_*.png` icons.
    pub icon_dir: String,
    pub main_window: Option<String>,
    pub wallpaper_host: Option<String>,
    pub splash_overlay: Option<String>,
    /// QML module directory (`Qcm/Material`, `waywallen/control`).
    pub qml_modules: Option<String>,
    pub filter_lib: Option<String>,
    /// The `rexon-priv-helper` privilege session helper ("one password per run").
    /// Not in the payload — the install doesn't install it and removes the old one:
    /// the window would reject a helper from a different build anyway.
    pub priv_helper: Option<String>,
}

impl PayloadSource {
    /// Unpacked archive: layout [`InstallLayout::bundle`], icons at the root. The
    /// archive layout is a proposal for block H.
    pub fn from_bundle<IO: SystemIo + ?Sized>(io: &IO, root: &str) -> Self {
        let l = InstallLayout::bundle(root);
        let file = |p: String| io.is_file(&p).then_some(p);
        let qml = l.qml_modules();
        PayloadSource {
            package_dir: l.package_dir(),
            icon_dir: l.lib_dir.clone(),
            main_window: file(l.main_window()),
            wallpaper_host: file(l.wallpaper_host()),
            splash_overlay: file(l.splash_overlay()),
            qml_modules: io.is_dir(&qml).then_some(qml),
            filter_lib: file(l.filter_lib()),
            priv_helper: file(format!("{}/{}", l.bin_dir, plan::SESSION_HELPER_NAME)),
        }
    }

    /// Developer tree: binaries — from the build directory next to the window
    /// binary, QML modules — from the user directory where
    /// `tools/build_qml_*.sh` puts them.
    pub fn from_dev_tree<IO: SystemIo + ?Sized>(
        io: &IO,
        root: &str,
        home: &str,
        xdg_data_home: Option<&str>,
    ) -> Self {
        let root = root.trim_end_matches('/');
        let release = inst::DEV_TREE_MAIN_WINDOW_REL
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");
        let built = |name: &str| {
            let p = format!("{root}/{release}/{name}");
            io.is_file(&p).then_some(p)
        };
        let qml_modules = [utils::data_home(xdg_data_home, home), utils::data_home(None, home)]
            .into_iter()
            .map(|base| format!("{base}/{}", qm::MODULES_REL))
            .find(|dir| io.is_file(&format!("{dir}/{}", qm::QCM_MATERIAL_REL)));
        PayloadSource {
            package_dir: format!("{root}/{}", inst::DEV_TREE_DATA_DIR),
            icon_dir: root.to_string(),
            main_window: built(crate::window_menu::MAIN_WINDOW_BIN),
            wallpaper_host: built(crate::wallpaper_engine::WALLPAPER_HOST_BINARY),
            splash_overlay: built(crate::session_splash::SPLASH_OVERLAY_BINARY),
            qml_modules,
            filter_lib: built(qm::FILTER_LIB_NAME),
            priv_helper: built(plan::SESSION_HELPER_NAME),
        }
    }

    /// Payload of the currently running Rexon: the developer tree if the tree root
    /// is known (`cli::repo_root`), otherwise — the archive around its own binary.
    pub fn for_running<IO: SystemIo + ?Sized>(
        io: &IO,
        own_exe: &str,
        repo_root: &str,
        home: &str,
        xdg_data_home: Option<&str>,
    ) -> Self {
        if !repo_root.is_empty() {
            return Self::from_dev_tree(io, repo_root, home, xdg_data_home);
        }
        let dir = own_exe.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        Self::from_bundle(io, dir)
    }
}

/// The former library of the `rexon-gui` crate (cdylib, 54 MB). It was loaded
/// via ctypes by the Python tab host; that is archived, the crate is now only
/// an rlib and statically linked into the window. The package and the
/// installer no longer ship it, but in the developer's QML module directory
/// (`~/.local/share/rexon/qml_modules`, where the
/// [`PayloadSource::from_dev_tree`] payload takes from) an old copy lies until
/// `tools/build_qml_control.sh` rebuilds the module. Without skipping it the
/// install would carry dead weight into the system.
const RETIRED_GUI_LIB: &str = "librexon_gui.so";

/// Not copied into the install: Python sources, bytecode caches and drafts.
///
/// ⭐ Fix of 16.09.2026 (rule #16): the manual install put the WHOLE Python
/// package into the system — the code of the classic window the payload no
/// longer carries. Now, as with the distribution package
/// (`packaging/aur/PKGBUILD`: `find "$data" -name '*.py' -delete`), only the
/// product data goes: vendored QML of the wallpaper tab, `themes.json`, fonts,
/// the Plasma wallpaper plugin and the GNOME and Cinnamon add-ons. The data
/// directory marker (`installer::PACKAGE_MARKER_REL`) is a QML file, not a
/// `.py`, so the lookup still recognises the layout.
///
/// [`RETIRED_GUI_LIB`] — skipped too: see there.
///
/// ⚠️ The description translation model (`translate/<pair>/model.….bin` and
/// the vocabulary, 43 MB) is deliberately NOT skipped: the data directory is
/// copied whole, and the model must travel along with it — otherwise the
/// manual install would silently be left without description translation,
/// although the distribution package ships the model
/// (`packaging/aur/PKGBUILD`). The developer tree has no model, there is
/// nothing to take from there; from an unpacked archive it travels by itself.
pub fn payload_skip(name: &str) -> bool {
    name == "__pycache__"
        || name == RETIRED_GUI_LIB
        || name.ends_with(".py")
        || name.ends_with(".pyc")
        || name.ends_with(".bak")
        || name.ends_with(".draft")
}

/// Permissions in the install: directories and executables — 755, the rest — 644.
/// `executable` — whether the source has the execute bit.
///
/// Fix 4: the old `chmod -R 755` made every `.py`, `.qml` and picture
/// executable.
pub fn payload_mode(is_dir: bool, executable: bool) -> u32 {
    if is_dir || executable {
        0o755
    } else {
        0o644
    }
}

// ============================================================================
// Executor trait
// ============================================================================

/// Install side effects BEYOND [`SystemIo`]. No decisions.
pub trait InstallIo: SystemIo {
    /// Create a unique temporary directory (0700) in `parent` (`None` — the
    /// system temporary directory) → path.
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String>;
    /// Rename a file or directory (one file system). A link is renamed itself,
    /// its target is not entered.
    fn rename(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Create a directory with all parents. `mode` — permissions for each
    /// CREATED directory (`None` — leave alone, like `os.makedirs`).
    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String>;
    /// Write a whole file and set permissions.
    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String>;
    /// Copy a file (content) and set permissions.
    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String>;
    /// Copy a tree. Names for which `skip` is true are skipped together with
    /// their content; permissions of each directory and file — `mode(is_dir,
    /// executable)`. A symlink to a file is copied as content, a symlink to a
    /// directory and a dangling symlink are skipped. → number of files copied.
    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        skip: &dyn Fn(&str) -> bool,
        mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String>;
    /// Delete a file; errors don't matter.
    fn remove_file(&self, path: &str);
    /// Delete a whole directory; errors don't matter.
    fn remove_tree(&self, path: &str);
}

// ============================================================================
// Request and outcome
// ============================================================================

/// What is requested to be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub payload: PayloadSource,
    pub family: PkgFamily,
    /// `ID` from os-release: on an immutable system the install refuses before
    /// the first side effect (the "then into the home folder" choice — G3b).
    pub distro_id: String,
    /// Value of `REXON_ENTRY` (fallback to the classic window).
    pub entry_env: Option<String>,
    /// The person EXPLICITLY agreed to the "no repeated password" rule.
    pub passwordless_rule: bool,
    /// Language of the progress and summary strings.
    pub lang: Lang,
}

impl InstallRequest {
    pub fn new(payload: PayloadSource, family: PkgFamily, distro_id: &str, lang: Lang) -> Self {
        InstallRequest {
            payload,
            family,
            distro_id: distro_id.to_string(),
            entry_env: None,
            passwordless_rule: inst::PASSWORDLESS_CONSENT_DEFAULT,
            lang,
        }
    }
}

/// How the install ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    /// Result for the summary page (`installer::result_texts`). `None` —
    /// cancelled by the person before the password prompt, the system untouched.
    pub outcome: Option<WizardOutcome>,
    /// Which entry is written into the desktop entry (present if it got that far).
    pub entry: Option<LaunchEntry>,
    /// One summary line — for the log and `(ok, message)`.
    pub message: String,
}

impl InstallResult {
    pub fn ok(&self) -> bool {
        matches!(self.outcome, Some(WizardOutcome::Installed { .. }))
    }

    pub fn cancelled(&self) -> bool {
        self.outcome.is_none()
    }

    pub fn failure(&self) -> Option<&InstallFailure> {
        match &self.outcome {
            Some(WizardOutcome::Failed(f)) => Some(f),
            _ => None,
        }
    }
}

/// Preparation stage (doesn't change the progress window title; titles are G5's business).
pub const STAGE_PREPARE: &str = "app_prepare";
/// Root script stage.
pub const STAGE_INSTALL: &str = "app_install";
/// Summary stage (present in `de_progress::STAGE_TITLES`).
pub const STAGE_DONE: &str = "done";

// ============================================================================
// Root script and plan
// ============================================================================

/// Step start marker in the script output (stdout).
pub const STEP_MARK: &str = "REXON-STEP:";
/// Marker of the step that broke (stderr, from the ERR trap).
pub const FAIL_MARK: &str = "REXON-FAILED:";
/// Prefix of the install temporary directory.
pub const STAGE_PREFIX: &str = "rexon_install_";
/// Desktop entry in the application menu.
pub const DESKTOP_FILE: &str = "rexon.desktop";
/// Desktop entry of the emergency appearance rollback.
pub const RECOVER_DESKTOP_FILE: &str = "rexon-recover.desktop";

/// Root script step markers → install step for the person. The "no password"
/// rule is a SEPARATE marker, even though for the person it is the same "fewer passwords" step.
pub const SCRIPT_MARKS: &[(&str, InstallStep)] = &[
    ("deps", InstallStep::Dependencies),
    ("files", InstallStep::CopyFiles),
    ("launcher", InstallStep::Launcher),
    ("menu", InstallStep::MenuShortcuts),
    ("icons", InstallStep::Icons),
    ("helpers", InstallStep::FewerPasswords),
    ("rule", InstallStep::FewerPasswords),
    ("caches", InstallStep::MenuRefresh),
    ("extensions", InstallStep::WallpaperAddons),
];

fn mark_step(id: &str) -> Option<InstallStep> {
    SCRIPT_MARKS.iter().find(|(m, _)| *m == id.trim()).map(|(_, s)| *s)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Layout of the install temporary directory. Everything temporary is inside
/// one directory, so cleanup is a single deletion (fix 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePaths {
    pub root: String,
}

impl StagePaths {
    pub fn new(root: &str) -> Self {
        StagePaths { root: root.trim_end_matches('/').to_string() }
    }

    /// The same layout as the target: `lib/` goes to `lib_dir`, `bin/` — to
    /// `bin_dir`.
    pub fn layout(&self) -> InstallLayout {
        InstallLayout::new(&format!("{}/lib", self.root), &format!("{}/bin", self.root))
    }

    pub fn menu_dir(&self) -> String {
        format!("{}/menu", self.root)
    }

    pub fn desktop(&self) -> String {
        format!("{}/{DESKTOP_FILE}", self.menu_dir())
    }

    pub fn recover_desktop(&self) -> String {
        format!("{}/{RECOVER_DESKTOP_FILE}", self.menu_dir())
    }

    pub fn icons_dir(&self) -> String {
        format!("{}/icons", self.root)
    }

    pub fn polkit_dir(&self) -> String {
        format!("{}/polkit", self.root)
    }

    /// Helpers, policies and the rule — under the names of their destinations.
    pub fn helpers(&self) -> HelperTmp {
        let p = |dest: &str| format!("{}/{}", self.polkit_dir(), basename(dest));
        HelperTmp {
            tmp_helper: p(inst::SYSTEM_HELPER_PATH),
            tmp_policy: p(inst::SYSTEM_POLICY_PATH),
            tmp_de_helper: p(plan::DE_HELPER_PATH),
            tmp_de_policy: p(plan::DE_POLICY_PATH),
            tmp_pkg_helper: p(plan::PKG_HELPER_PATH),
            tmp_pkg_policy: p(plan::PKG_POLICY_PATH),
            tmp_stage_helper: p(plan::STAGE_HELPER_PATH),
            tmp_stage_policy: p(plan::STAGE_POLICY_PATH),
            tmp_de_rules: p(plan::REXON_POLKIT_RULES_PATH),
        }
    }

    /// The privilege session helper in the temporary directory (a copy of the payload binary).
    pub fn priv_helper(&self) -> String {
        format!("{}/{}", self.polkit_dir(), plan::SESSION_HELPER_NAME)
    }

    /// The `org.rexon.session` policy in the temporary directory.
    pub fn session_policy(&self) -> String {
        format!("{}/{}", self.polkit_dir(), basename(plan::SESSION_POLICY_PATH))
    }

    pub fn script(&self) -> String {
        format!("{}/install.sh", self.root)
    }
}

/// The temporary directory path is fit for the script without quotes: the
/// core's icon and helper blocks substitute paths as is.
fn stage_path_is_plain(path: &str) -> bool {
    path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

/// Everything the root script is assembled from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemScriptParams {
    pub target: InstallLayout,
    pub desktop_dir: String,
    /// hicolor icon root (for refreshing the icon cache).
    pub icon_root: String,
    pub gnome_ext_dir: String,
    pub cinnamon_ext_dir: String,
    pub stage: StagePaths,
    pub with_wallpaper_host: bool,
    pub with_splash_overlay: bool,
    /// The payload has the privilege session helper ([`PayloadSource::priv_helper`]).
    pub with_priv_helper: bool,
    /// Icons in the temporary directory.
    pub icons: IconSet,
    pub passwordless_rule: bool,
    /// Dependency install lines ([`deps_script_lines`]).
    pub dep_lines: String,
}

/// Dependency install lines for the root script — port of
/// `_install_deps_cmd`. There are three tiers, each on its own line:
///
///   1. REQUIRED ([`inst::dep_packages`]) — strictly, under `set -e`: without
///      Qt6, layer-shell, the Vulkan loader and libmpv the window won't open,
///      and an honest refusal is better than a silently broken product.
///   2. RECOMMENDED ([`inst::optional_dep_packages`]) — with `|| true`: this is
///      the base (bash, D-Bus, Wayland, GL, OpenSSL, polkit, an icon theme)
///      that is almost always present on a machine with a desktop, and its
///      package names in eleven families out of twelve are written out by
///      naming rules rather than verified live. A typo here must not cost the
///      person the whole install.
///   3. THE Qt STYLE SWITCHER ([`inst::qt_style_install_cmd`]) — also with
///      `|| true`: Rexon works without it.
///
/// ⚠️ Recommended ones are installed exactly when the required ones are
/// (`deps_present` — one answer of the `ldd` probe for the whole payload): if
/// the linker is happy, the system has both, and there is no reason to call
/// the package manager.
pub fn deps_script_lines(family: PkgFamily, deps_present: bool, qt_style_present: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !deps_present {
        let cmd = inst::dep_install_cmd(family);
        if !cmd.is_empty() {
            lines.push(cmd);
        }
        let optional = inst::optional_dep_install_cmd(family);
        if !optional.is_empty() {
            lines.push(format!("{optional} || true"));
        }
    }
    if !qt_style_present {
        let cmd = inst::qt_style_install_cmd(family);
        if !cmd.is_empty() {
            lines.push(format!("{cmd} || true"));
        }
    }
    lines.join("\n")
}

/// GNOME Shell extension directory inside product data `rexon/`.
pub const GNOME_EXTENSION_SRC: &str = "gnome-extension";
/// Cinnamon extension directory inside product data `rexon/`.
pub const CINNAMON_EXTENSION_SRC: &str = "cinnamon-extension";

/// Pairs (extension directory in the package, where to install it) — the same
/// for the system and home folder installs. Mutter and Muffin can't do
/// wlr-layer-shell: the wallpaper there is drawn by the extension; it is
/// always installed — in case of switching to GNOME or Cinnamon.
pub fn extension_targets(gnome_ext_dir: &str, cinnamon_ext_dir: &str) -> [(&'static str, String); 2] {
    [
        (GNOME_EXTENSION_SRC, gnome_ext_dir.trim_end_matches('/').to_string()),
        (CINNAMON_EXTENSION_SRC, cinnamon_ext_dir.trim_end_matches('/').to_string()),
    ]
}

/// Program that compiles the extension's settings schemas.
pub const GLIB_COMPILE_SCHEMAS: &str = "glib-compile-schemas";
/// Program that updates the menu desktop entry database.
pub const UPDATE_DESKTOP_DATABASE: &str = "update-desktop-database";
/// Ceiling of one cache refresh command, s.
pub const CACHE_TIMEOUT_SECS: u64 = 120;

/// Refreshing menu and icon caches — the same commands for both modes. A group
/// is interchangeable commands in order: the first successful one closes the
/// group. No program of the group present — not an error.
pub fn cache_refresh_groups(desktop_dir: &str, icon_root: &str) -> Vec<Vec<Vec<String>>> {
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    vec![
        vec![argv(&[UPDATE_DESKTOP_DATABASE, desktop_dir])],
        vec![
            argv(&["gtk4-update-icon-cache", "-f", "-t", icon_root]),
            argv(&["gtk-update-icon-cache", "-f", "-t", icon_root]),
        ],
        vec![argv(&["kbuildsycoca6", "--noincremental"]), argv(&["kbuildsycoca5", "--noincremental"])],
    ]
}

/// Text of the system install root script.
pub fn install_script(p: &SystemScriptParams) -> String {
    let q = |s: &str| crate::ext_manager::shlex_quote(s);
    let stage = p.stage.layout();
    let lib = q(&p.target.lib_dir);
    let desktop = q(&p.desktop_dir);
    let mut lines: Vec<String> = Vec::new();
    let mut line = |l: String| lines.push(l);
    let step = |id: &str| format!("\nrexon_step {id}");

    line("#!/bin/bash".into());
    line("# Installing Rexon into the system. The script is built by the Rexon core.".into());
    line("set -eE".into());
    line("umask 022".into());
    // No terminal: a command that asked for input would wait forever.
    line("exec </dev/null".into());
    line("REXON_STEP=start".into());
    line(format!("trap 'echo \"{FAIL_MARK}$REXON_STEP\" >&2' ERR"));
    line(format!("rexon_step() {{ REXON_STEP=\"$1\"; echo \"{STEP_MARK}$1\"; }}"));

    line(step("deps"));
    line(if p.dep_lines.is_empty() {
        "# Required programs are already installed".into()
    } else {
        p.dep_lines.clone()
    });

    line(step("files"));
    line(format!("rm -rf {lib}"));
    line(format!("install -d -m755 {lib}"));
    // `dir/.` — with hidden files; permissions were already laid out during preparation.
    line(format!("cp -r --preserve=mode {}/. {lib}/", q(&stage.lib_dir)));
    line(format!("chown -R root:root {lib}"));

    line(step("launcher"));
    let mut bins = vec![(stage.launcher(), p.target.launcher())];
    if p.with_wallpaper_host {
        bins.push((stage.wallpaper_host(), p.target.wallpaper_host()));
    }
    if p.with_splash_overlay {
        bins.push((stage.splash_overlay(), p.target.splash_overlay()));
    }
    for (src, dst) in bins {
        line(format!("install -Dm755 {} {}", q(&src), q(&dst)));
    }

    line(step("menu"));
    line(format!("install -Dm644 {} {desktop}/rexon.desktop", q(&p.stage.desktop())));
    line(format!(
        "install -Dm644 {} {desktop}/rexon-recover.desktop",
        q(&p.stage.recover_desktop())
    ));

    line(step("icons"));
    line(inst::icon_install_block(&p.icons));

    let h = p.stage.helpers();
    line(step("helpers"));
    line(inst::helper_install_block(&h));
    // The privilege session helper and its policy (`org.rexon.session`). The
    // binary and the directory above it are root's, not writable by group and
    // others: otherwise the window won't start the helper
    // (`privilege::client::check_ownership`). No helper in the payload — the old
    // one is removed: the window would reject one from a different build anyway,
    // and without the file operations take the old path.
    if p.with_priv_helper {
        line(format!("install -Dm755 {} {}", q(&p.stage.priv_helper()), plan::SESSION_HELPER_PATH));
        line(format!("install -Dm644 {} {}", q(&p.stage.session_policy()), plan::SESSION_POLICY_PATH));
    } else {
        line(format!("rm -f {}", plan::SESSION_HELPER_PATH));
    }
    if p.passwordless_rule {
        line(step("rule"));
    }
    line(inst::passwordless_rule_block(&h.tmp_de_rules, p.passwordless_rule));

    line(step("caches"));
    for group in cache_refresh_groups(&p.desktop_dir, &p.icon_root) {
        let alternatives: Vec<String> = group
            .iter()
            .map(|argv| format!("{} 2>/dev/null", argv.iter().map(|a| q(a)).collect::<Vec<_>>().join(" ")))
            .collect();
        line(format!("{} || true", alternatives.join(" || ")));
    }

    line(step("extensions"));
    let pairs: Vec<String> = extension_targets(&p.gnome_ext_dir, &p.cinnamon_ext_dir)
        .iter()
        .map(|(src, dest)| q(&format!("{src}|{dest}")))
        .collect();
    line(format!("for de_pair in {}; do", pairs.join(" ")));
    line("    DE_DIR=\"${de_pair%|*}\"".into());
    line("    DEST_BASE=\"${de_pair#*|}\"".into());
    line(format!("    EXT_SRC={}/\"$DE_DIR\"/{}", q(&p.target.package_dir()), inst::EXT_ID));
    line(format!("    EXT_DEST=\"$DEST_BASE\"/{}", inst::EXT_ID));
    line(format!("    rm -rf \"$DEST_BASE\"/{}", inst::EXT_ID_LEGACY));
    line("    if [ -d \"$EXT_SRC\" ]; then".into());
    line("        rm -rf \"$EXT_DEST\"".into());
    line("        mkdir -p \"$DEST_BASE\"".into());
    line("        cp -r --preserve=mode \"$EXT_SRC\" \"$EXT_DEST\"".into());
    line("        chown -R root:root \"$EXT_DEST\"".into());
    line(
        format!("        if command -v {GLIB_COMPILE_SCHEMAS} >/dev/null 2>&1 && [ -d \"$EXT_DEST/schemas\" ]; then"),
    );
    line(format!("            {GLIB_COMPILE_SCHEMAS} \"$EXT_DEST/schemas/\" 2>/dev/null || true"));
    line("        fi".into());
    line("    fi".into());
    line("done".into());
    lines.join("\n") + "\n"
}

/// Command running the root script — the same form as
/// [`inst::system_install_cmd`] (argv and a 600 s timeout).
pub fn install_cmd(script_path: &str) -> Cmd {
    inst::system_install_cmd(script_path)
}

/// Plan: write the script → run it under a password → remove the temporary
/// directory. The executor does the cleanup on ANY outcome, not only after a
/// successful run.
pub fn install_plan(p: &SystemScriptParams) -> Vec<PlanStep> {
    let script = p.stage.script();
    vec![
        PlanStep::WriteFile { path: script.clone(), content: install_script(p), mode: 0o700 },
        PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: install_cmd(&script),
            needs_root: true,
            description: InstallStep::SystemInstall.progress_line(Lang::Ru),
        },
        PlanStep::RemoveFiles { paths: vec![p.stage.root.clone()] },
    ]
}

fn check_plan(steps: &[PlanStep]) -> bool {
    steps.iter().all(|s| {
        matches!(
            s,
            PlanStep::WriteFile { .. }
                | PlanStep::RemoveFiles { .. }
                | PlanStep::Run { id: RunStepId::AppInstall, .. }
        )
    })
}

// ============================================================================
// Parsing the script outcome
// ============================================================================

/// Markers: steps started, in order, and the step from the ERR trap.
pub fn script_marks(out: &RunOut) -> (Vec<InstallStep>, Option<InstallStep>) {
    let mut started = Vec::new();
    let mut failed = None;
    for l in out.stdout.lines().chain(out.stderr.lines()) {
        let l = l.trim();
        if let Some(id) = l.strip_prefix(STEP_MARK) {
            if let Some(st) = mark_step(id) {
                started.push(st);
            }
        } else if let Some(id) = l.strip_prefix(FAIL_MARK) {
            failed = mark_step(id);
        }
    }
    (started, failed)
}

fn script_started(out: &RunOut) -> bool {
    out.stdout.lines().chain(out.stderr.lines()).any(|l| l.trim().starts_with(STEP_MARK))
}

/// The last output lines without markers — details for the developer.
pub fn output_tail(out: &RunOut, max_lines: usize) -> String {
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .chain(out.stderr.lines())
        .map(str::trim_end)
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with(STEP_MARK) && !t.starts_with(FAIL_MARK)
        })
        .collect();
    let from = lines.len().saturating_sub(max_lines);
    lines[from..].join("\n")
}

fn detail_with_code(code: Option<i32>, tail: &str) -> String {
    let head = match code {
        Some(c) => trf("exit code %1", &[&c]),
        None => tr("no exit code"),
    };
    if tail.is_empty() {
        head
    } else {
        format!("{head}\n{tail}")
    }
}

/// Outcome of running the root script.
///
/// Fix 1: "password not confirmed" — only if the script did NOT start (no
/// markers) and pkexec refused (code 126/127 or the agent's text —
/// [`plan::pkexec_auth_denied`]). If there are markers, it is a step failure,
/// even with code 127 from a command inside the script. No way to ask for a
/// password (the run didn't happen) — [`InstallFailure::PasswordPromptMissing`];
/// timeout — [`InstallFailure::TimedOut`].
pub fn classify_script_run(out: &RunOut, timeout_secs: u64) -> Result<(), InstallFailure> {
    match out.outcome {
        RunOutcome::Error => return Err(InstallFailure::PasswordPromptMissing),
        RunOutcome::Timeout => return Err(InstallFailure::TimedOut { secs: timeout_secs }),
        _ => {}
    }
    let code = out.returncode;
    if code == Some(0) {
        return Ok(());
    }
    let tail = output_tail(out, 15);
    if script_started(out) {
        let (started, failed) = script_marks(out);
        let step = failed.or(started.last().copied()).unwrap_or(InstallStep::SystemInstall);
        return Err(InstallFailure::StepFailed { step, detail: detail_with_code(code, &tail) });
    }
    let c = code.unwrap_or(-1);
    if c == 127 || plan::pkexec_auth_denied(c, &out.stderr) {
        return Err(InstallFailure::PasswordDenied);
    }
    Err(InstallFailure::StepFailed {
        step: InstallStep::SystemInstall,
        detail: detail_with_code(code, &tail),
    })
}

// ============================================================================
// Executor
// ============================================================================

/// Program that shows a binary's dynamic dependencies.
const LDD: &str = "ldd";
/// `ldd`'s answer about an unresolved library (the wording is English: it runs
/// with `lang_c`, otherwise a translated message would throw the check off).
const LDD_MISSING_MARK: &str = "not found";
/// Ceiling of one probe, s: `ldd` only reads the file headers.
const LDD_TIMEOUT_SECS: u64 = 20;

/// The "payload will start" probe: whether the dynamic dependencies resolve
/// for the very files the install is about to put into the system — the
/// window, the wallpaper host, the login splash and the filter model library.
///
/// ⚠️ Previously the interpreter was asked here (`python3 -c "import
/// PySide6.QtWidgets"`) — that was a question about the classic window the
/// payload no longer carries: the Rust window doesn't need PySide6 at all,
/// and the probe itself would fail on a machine without the interpreter and
/// call the package manager for nothing. We ask exactly what the Rust payload
/// lives on: Qt6 libraries, layer-shell, libmpv, Vulkan — all visible to the
/// dynamic linker.
///
/// Nothing to check with (the payload has no binaries, or the system has no
/// `ldd`) — we answer "present": a silent "no" would call the package manager
/// blindly, and on a machine without network that would abort the whole install.
fn runtime_deps_present<IO: InstallIo + ?Sized>(io: &IO, payload: &PayloadSource) -> bool {
    let files: Vec<&String> = [
        payload.main_window.as_ref(),
        payload.wallpaper_host.as_ref(),
        payload.splash_overlay.as_ref(),
        payload.filter_lib.as_ref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if files.is_empty() || !io.which(LDD) {
        return true;
    }
    files.iter().all(|path| {
        let argv: Vec<String> = vec![LDD.to_string(), (*path).clone()];
        let out = io.run(&argv, Some(LDD_TIMEOUT_SECS), true);
        out.returncode == Some(0)
            && !out.stdout.contains(LDD_MISSING_MARK)
            && !out.stderr.contains(LDD_MISSING_MARK)
    })
}

/// Desktop entry target AFTER the install: the rule is the same as on the live
/// machine, the candidate is the window binary in its future place.
pub fn installed_entry(entry_env: Option<&str>, target: &InstallLayout, has_main_window: bool) -> LaunchEntry {
    inst::choose_launch_entry(entry_env, &[target.main_window()], |_| has_main_window)
}

/// Payload icons — source paths. The master copy is taken only if there are
/// no large sizes.
fn payload_icons<IO: SystemIo + ?Sized>(io: &IO, icon_dir: &str) -> IconSet {
    let dir = icon_dir.trim_end_matches('/');
    let mut set = IconSet::default();
    for size in ICON_SIZES {
        let src = format!("{dir}/rexon_icon_{size}.png");
        if io.is_file(&src) {
            set.sizes.push((*size, src));
        }
    }
    let svg = format!("{dir}/rexon_icon.svg");
    if io.is_file(&svg) {
        set.svg = Some(svg);
    }
    let has_big = set.sizes.iter().any(|(s, _)| matches!(s, 128 | 256 | 512));
    let master = format!("{dir}/rexon_icon.png");
    if !has_big && io.is_file(&master) {
        set.master = Some(master);
    }
    set
}

/// Payload icons for the system install: the set in the temporary directory
/// for the core's install block + pairs (source, path in the temporary directory).
fn icon_plan<IO: SystemIo + ?Sized>(
    io: &IO,
    icon_dir: &str,
    stage: &StagePaths,
) -> (IconSet, Vec<(String, String)>) {
    let src = payload_icons(io, icon_dir);
    let mut set = IconSet::default();
    let mut copies = Vec::new();
    let mut stage_copy = |from: &String, name: String| {
        let dst = format!("{}/{name}", stage.icons_dir());
        copies.push((from.clone(), dst.clone()));
        dst
    };
    for (size, from) in &src.sizes {
        let dst = stage_copy(from, format!("rexon_icon_{size}.png"));
        set.sizes.push((*size, dst));
    }
    if let Some(from) = &src.svg {
        set.svg = Some(stage_copy(from, "rexon_icon.svg".into()));
    }
    if let Some(from) = &src.master {
        set.master = Some(stage_copy(from, "rexon_icon_master.png".into()));
    }
    (set, copies)
}

/// Script parameters for this machine and this temporary directory. The
/// executor calls exactly this — the checks build the same text.
pub fn system_script_params<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    stage: &StagePaths,
) -> SystemScriptParams {
    let sys = inst::install_paths(InstallMode::System, "", None);
    let (icons, _) = icon_plan(io, &req.payload.icon_dir, stage);
    let qt_style_present = io.which("qt6ct") || io.which("qt5ct");
    SystemScriptParams {
        target: InstallLayout::system(),
        desktop_dir: sys.desktop_dir.unwrap_or_default(),
        icon_root: sys.icon_root.unwrap_or_default(),
        gnome_ext_dir: sys.gnome_ext_dir.unwrap_or_default(),
        cinnamon_ext_dir: sys.cinnamon_ext_dir.unwrap_or_default(),
        stage: stage.clone(),
        with_wallpaper_host: req.payload.wallpaper_host.is_some(),
        with_splash_overlay: req.payload.splash_overlay.is_some(),
        with_priv_helper: req.payload.priv_helper.is_some(),
        icons,
        passwordless_rule: req.passwordless_rule,
        dep_lines: deps_script_lines(
            req.family,
            runtime_deps_present(io, &req.payload),
            qt_style_present,
        ),
    }
}

/// A step preparation error — a failure of that step, the detail is the error text.
fn prep<T>(step: InstallStep, r: Result<T, String>) -> Result<T, InstallFailure> {
    r.map_err(|detail| InstallFailure::StepFailed { step, detail })
}

/// Result of an install in ANY mode: the summary line into progress and a
/// check that the `rexon` command directory is in `PATH` (`command_dir` —
/// `None` when there is no command, as in portable mode). One for all modes.
fn finish<IO: InstallIo + ?Sized>(
    io: &IO,
    sink: &dyn DeOpSink,
    lang: Lang,
    mode: InstallMode,
    command_dir: Option<&str>,
    res: Result<Option<LaunchEntry>, Option<InstallFailure>>,
) -> InstallResult {
    match res {
        Ok(entry) => {
            let message = inst::install_done_line(mode, lang);
            sink.progress(STAGE_DONE, &message, Some(1.0));
            let path_env = io.env("PATH");
            let not_in_path = command_dir
                .filter(|dir| !inst::command_dir_in_path(dir, Some(&path_env)))
                .map(str::to_string);
            InstallResult {
                outcome: Some(WizardOutcome::Installed {
                    mode,
                    fell_back: None,
                    command_dir_not_in_path: not_in_path,
                }),
                entry,
                message,
            }
        }
        Err(None) => {
            let message = crate::system_de_exec::cancelled_message();
            sink.progress(crate::system_de_exec::STATUS_CANCELLED, &message, None);
            InstallResult { outcome: None, entry: None, message }
        }
        Err(Some(failure)) => {
            let message = failure.log_line(lang);
            sink.progress(de_progress::STAGE_ERROR, &message, None);
            InstallResult { outcome: Some(WizardOutcome::Failed(failure)), entry: None, message }
        }
    }
}

/// Settings and backup folders — the same for all modes (fix 8). Empty if the
/// home folder is not determined: a relative path would look into the
/// process's current directory. Paths come from the single list
/// [`inst::user_data_roots`]: the first XDG ones are settings and backups.
pub fn profile_dirs(home: &str, xdg_config_home: &str, xdg_data_home: &str) -> Vec<String> {
    let roots = inst::user_data_roots(home, xdg_config_home, xdg_data_home, "");
    [inst::UserDataKind::Settings, inst::UserDataKind::Backups]
        .into_iter()
        .filter_map(|kind| roots.iter().find(|(k, _)| *k == kind).map(|(_, p)| p.clone()))
        .collect()
}

/// Create the profile folders. No harm if it fails: Rust creates them itself on
/// first access.
fn make_profile_dirs<IO: InstallIo + ?Sized>(io: &IO) {
    for dir in profile_dirs(&io.env("HOME"), &io.env("XDG_CONFIG_HOME"), &io.env("XDG_DATA_HOME")) {
        let _ = io.make_dirs(&dir, None);
    }
}

/// Program files in `staged.lib_dir` with correct permissions: product data
/// `rexon/` (without Python sources — [`payload_skip`]), the window binary,
/// QML modules, our `.so`. One preparation for both the system and home
/// folder installs.
fn stage_program_files<IO: InstallIo + ?Sized>(
    io: &IO,
    pay: &PayloadSource,
    staged: &InstallLayout,
) -> Result<(), InstallFailure> {
    use InstallStep::CopyFiles;
    prep(CopyFiles, io.make_dirs(&staged.lib_dir, Some(0o755)))?;
    prep(CopyFiles, io.copy_tree(&pay.package_dir, &staged.package_dir(), &payload_skip, &payload_mode))?;
    if let Some(src) = &pay.main_window {
        prep(CopyFiles, io.copy_file_mode(src, &staged.main_window(), 0o755))?;
        // The style snapshot helper is built and lies next to the window in any
        // payload; without it the tiles get drawn samples.
        let helper = std::path::Path::new(src).with_file_name(crate::widget_shot::HELPER_BINARY);
        let helper = helper.to_string_lossy();
        if io.is_file(&helper) {
            prep(CopyFiles, io.copy_file_mode(&helper, &staged.theme_shot(), 0o755))?;
        }
    }
    if let Some(src) = &pay.qml_modules {
        prep(CopyFiles, io.copy_tree(src, &staged.qml_modules(), &payload_skip, &payload_mode))?;
    }
    if let Some(src) = &pay.filter_lib {
        prep(CopyFiles, io.make_dirs(&staged.rust_lib_dir(), Some(0o755)))?;
        prep(CopyFiles, io.copy_file_mode(src, &staged.filter_lib(), 0o755))?;
    }
    Ok(())
}

/// Install Rexon into the system. The window calls not this but the dispatcher
/// [`install`]: it redirects an immutable system to the home folder up front.
///
/// Order: refusal on an immutable system (before side effects) → temporary
/// directory → preparing files with correct permissions → the root script
/// under one password → checking that Rexon is in place → profile folders.
/// The temporary directory is removed on ANY outcome after it is created.
/// Cancelling is possible until the password is requested.
pub fn install_to_system<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    let lang = req.lang;
    let target = InstallLayout::system();
    let done = |res: Result<LaunchEntry, Option<InstallFailure>>| {
        finish(io, sink.as_ref(), lang, InstallMode::System, Some(target.bin_dir.as_str()), res.map(Some))
    };

    // Immutable system: we shouldn't get here — "into the home folder" is chosen
    // by the dispatcher [`install`]. Refusal — before the first side effect.
    if let Some(reason) = inst::system_mode_blocked_reason_in(&req.distro_id, lang) {
        return done(Err(Some(InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: reason,
        })));
    }
    sink.cancellable(true);
    sink.progress(STAGE_PREPARE, &InstallStep::CopyFiles.progress_line(lang), Some(0.02));
    let root = match io.temp_dir(STAGE_PREFIX, None) {
        Ok(r) => r,
        Err(e) => {
            sink.cancellable(false);
            return done(Err(Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: e })));
        }
    };
    let res = run_staged(io, req, sink.as_ref(), cancel, &StagePaths::new(&root));
    // Fix 2: cleanup on any outcome — success, password refusal, step failure,
    // preparation error, cancel, timeout.
    io.remove_tree(&root);
    sink.cancellable(false);
    done(res)
}

fn run_staged<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &InstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
    stage: &StagePaths,
) -> Result<LaunchEntry, Option<InstallFailure>> {
    let lang = req.lang;
    let say = |step: InstallStep, fraction: f64| {
        sink.progress(STAGE_PREPARE, &step.progress_line(lang), Some(fraction));
    };
    if !stage_path_is_plain(&stage.root) {
        return Err(Some(InstallFailure::StepFailed {
            step: InstallStep::CopyFiles,
            detail: trf("the temporary folder path has spaces or special characters: %1", &[&stage.root]),
        }));
    }
    let params = system_script_params(io, req, stage);
    let steps = install_plan(&params);
    if !check_plan(&steps) {
        return Err(Some(InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: tr("the installation plan contains an unknown step"),
        }));
    }
    let target = &params.target;
    let staged = stage.layout();
    let pay = &req.payload;
    let entry = installed_entry(req.entry_env.as_deref(), target, pay.main_window.is_some());
    let cancelled = || cancel.is_cancelled();

    // ── Program files ───────────────────────────────────────────────
    use InstallStep::*;
    prep(CopyFiles, io.make_dirs(&staged.bin_dir, Some(0o755)))?;
    stage_program_files(io, pay, &staged)?;
    if cancelled() {
        return Err(None);
    }

    // ── Launch commands ─────────────────────────────────────────────
    say(Launcher, 0.12);
    for (src, dst) in [
        (&pay.wallpaper_host, staged.wallpaper_host()),
        (&pay.splash_overlay, staged.splash_overlay()),
    ] {
        if let Some(src) = src {
            prep(Launcher, io.copy_file_mode(src, &dst, 0o755))?;
        }
    }
    prep(
        Launcher,
        io.write_file_mode(&staged.launcher(), &inst::system_launcher(&target.lib_dir), 0o755),
    )?;

    // ── Desktop entry ───────────────────────────────────────────────
    say(MenuShortcuts, 0.18);
    prep(MenuShortcuts, io.make_dirs(&stage.menu_dir(), Some(0o755)))?;
    prep(MenuShortcuts, io.write_file_mode(&stage.desktop(), &inst::desktop_content_for(&entry), 0o644))?;
    prep(
        MenuShortcuts,
        io.write_file_mode(&stage.recover_desktop(), &inst::recover_desktop_content_for(&entry), 0o644),
    )?;

    // ── Icons ───────────────────────────────────────────────────────
    say(Icons, 0.24);
    let (_, icon_copies) = icon_plan(io, &pay.icon_dir, stage);
    if icon_copies.is_empty() {
        sink.progress(de_progress::STAGE_WARNING, &inst::icons_missing_notice(lang), None);
    } else {
        prep(Icons, io.make_dirs(&stage.icons_dir(), Some(0o755)))?;
        for (src, dst) in &icon_copies {
            prep(Icons, io.copy_file_mode(src, dst, 0o644))?;
        }
    }

    // ── Helpers and (with consent) the rule ─────────────────────────
    say(FewerPasswords, 0.3);
    let h = stage.helpers();
    prep(FewerPasswords, io.make_dirs(&stage.polkit_dir(), Some(0o755)))?;
    let texts: [(&str, String, u32); 8] = [
        (&h.tmp_helper, inst::system_helper_script(), 0o755),
        (&h.tmp_policy, inst::system_policy_xml(), 0o644),
        (&h.tmp_de_helper, plan::de_helper_script(), 0o755),
        (&h.tmp_de_policy, plan::de_policy_xml(), 0o644),
        (&h.tmp_pkg_helper, plan::pkg_helper_script(), 0o755),
        (&h.tmp_pkg_policy, plan::pkg_policy_xml(), 0o644),
        (&h.tmp_stage_helper, plan::stage_helper_script(), 0o755),
        (&h.tmp_stage_policy, plan::stage_policy_xml(), 0o644),
    ];
    for (path, text, mode) in &texts {
        prep(FewerPasswords, io.write_file_mode(path, text, *mode))?;
    }
    if let Some(src) = &pay.priv_helper {
        prep(FewerPasswords, io.copy_file_mode(src, &stage.priv_helper(), 0o755))?;
        prep(FewerPasswords, io.write_file_mode(&stage.session_policy(), &plan::session_policy_xml(), 0o644))?;
    }
    if req.passwordless_rule {
        prep(
            FewerPasswords,
            io.write_file_mode(&h.tmp_de_rules, &plan::passwordless_rules_js(req.family), 0o644),
        )?;
    }
    if !params.dep_lines.is_empty() {
        sink.progress(STAGE_PREPARE, &inst::dependencies_with_rexon_notice(lang), None);
    }
    if cancelled() {
        return Err(None);
    }

    // ── Plan: script → password ─────────────────────────────────────
    let mut run_out: Option<(RunOut, u64)> = None;
    for step in &steps {
        match step {
            PlanStep::WriteFile { path, content, mode } => {
                prep(SystemInstall, io.write_file_mode(path, content, *mode))?;
            }
            PlanStep::Run { cmd, .. } => {
                sink.cancellable(false);
                sink.progress(STAGE_INSTALL, &SystemInstall.progress_line(lang), Some(0.45));
                let out = io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                run_out = Some((out, cmd.timeout_secs.unwrap_or(inst::SYSTEM_INSTALL_TIMEOUT_SECS)));
            }
            // Cleanup is done by `install_to_system` on any outcome.
            PlanStep::RemoveFiles { .. } => {}
            _ => {
                return Err(Some(InstallFailure::StepFailed {
                    step: SystemInstall,
                    detail: tr("the installation plan contains an unknown step"),
                }))
            }
        }
    }
    let (out, timeout) = run_out.unwrap_or_else(|| (RunOut::missing(), inst::SYSTEM_INSTALL_TIMEOUT_SECS));
    // Log: the steps the script completed.
    let verdict = classify_script_run(&out, timeout);
    let (started, _) = script_marks(&out);
    let finished = if verdict.is_ok() { started.len() } else { started.len().saturating_sub(1) };
    let mut last: Option<InstallStep> = None;
    for st in started.iter().take(finished) {
        if last != Some(*st) {
            sink.progress(STAGE_INSTALL, &format!("✅ {}", st.progress_line(lang).trim_end_matches('…')), None);
        }
        last = Some(*st);
    }
    verdict.map_err(Some)?;

    // ── Check: Rexon is in place ────────────────────────────────────
    let sys = inst::install_paths(InstallMode::System, "", None);
    let in_place = inst::system_or_user_installed(&sys, |p| io.exists(p), |p| io.is_dir(p))
        && match &entry {
            LaunchEntry::MainWindow(path) => io.is_executable(path),
            LaunchEntry::Python => true,
        };
    if !in_place {
        return Err(Some(InstallFailure::StepFailed {
            step: SystemInstall,
            detail: trf(
                "the administrator script finished successfully, but %1 or %2 was not found",
                &[
                    &target.launcher(),
                    &match &entry {
                        LaunchEntry::MainWindow(p) => p.clone(),
                        LaunchEntry::Python => target.package_dir(),
                    },
                ],
            ),
        }));
    }

    // ── Profile folders (no password) ───────────────────────────────
    sink.progress(STAGE_INSTALL, &UserFolders.progress_line(lang), Some(0.95));
    make_profile_dirs(io);
    Ok(entry)
}

// ============================================================================
// Home folder install, portable mode and the dispatcher
// ============================================================================

/// Prefix of the home folder install temporary directory: hidden and next to
/// the install directory — the same file system, the swap is a single
/// rename (fix 9).
pub const USER_STAGE_PREFIX: &str = ".rexon_install_";

/// What is requested to be installed into the home folder — part of
/// [`InstallRequest`]: this mode has no password, dependencies or "no password" rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInstallRequest {
    pub payload: PayloadSource,
    /// Value of `REXON_ENTRY` (fallback to the classic window).
    pub entry_env: Option<String>,
    /// Language of the progress and summary strings.
    pub lang: Lang,
}

impl UserInstallRequest {
    pub fn new(payload: PayloadSource, lang: Lang) -> Self {
        UserInstallRequest { payload, entry_env: None, lang }
    }
}

impl InstallRequest {
    /// The same request for the home folder install.
    pub fn for_user(&self) -> UserInstallRequest {
        UserInstallRequest {
            payload: self.payload.clone(),
            entry_env: self.entry_env.clone(),
            lang: self.lang,
        }
    }
}

fn parent_dir(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    }
}

/// An existing path leads somewhere other than where it lies — it is a link
/// (for the developer `~/.local/lib/rexon` is sometimes a link into the tree).
/// Compared via the real path of the PARENT: on Bazzite and Silverblue `/home`
/// itself is a link to `/var/home`, and that is no reason to refuse.
pub fn is_redirected<IO: SystemIo + ?Sized>(io: &IO, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    if !io.exists(path) {
        return false;
    }
    match (io.realpath(path), io.realpath(parent_dir(path))) {
        (Some(real), Some(parent)) => real != format!("{}/{}", parent.trim_end_matches('/'), basename(path)),
        _ => false,
    }
}

/// Install a file via a temporary name next to it and a rename: a running
/// binary can't be opened for overwriting (`ETXTBSY`), and a rename replaces a
/// link in place without entering its target (fixes 12, 13).
fn place_file<IO: InstallIo + ?Sized>(io: &IO, src: &str, dst: &str, mode: u32) -> Result<(), String> {
    let tmp = format!("{dst}.rexon-new");
    io.copy_file_mode(src, &tmp, mode).and_then(|()| io.rename(&tmp, dst)).inspect_err(|_| {
        io.remove_file(&tmp);
    })
}

/// Write a text file in place of an existing one: the old path is removed
/// first so the write doesn't go into the link's target (fix 12).
fn replace_text<IO: InstallIo + ?Sized>(io: &IO, path: &str, content: &str, mode: u32) -> Result<(), String> {
    io.remove_file(path);
    io.write_file_mode(path, content, mode)
}

/// Menu and icon caches per [`cache_refresh_groups`]. A missing program is not
/// an error; without `update-desktop-database` the person learns that the
/// desktop entry will appear after logging in again.
/// `sink` — `None` for uninstall: there "the entry will appear later" would be untrue.
fn refresh_caches<IO: InstallIo + ?Sized>(
    io: &IO,
    sink: Option<&dyn DeOpSink>,
    lang: Lang,
    desktop_dir: &str,
    icon_root: &str,
) {
    for group in cache_refresh_groups(desktop_dir, icon_root) {
        let present: Vec<&Vec<String>> = group.iter().filter(|argv| io.which(&argv[0])).collect();
        if present.is_empty() {
            if let (Some(sink), true) = (sink, group[0][0] == UPDATE_DESKTOP_DATABASE) {
                sink.progress(de_progress::STAGE_WARNING, &inst::menu_refresh_later_notice(lang), None);
            }
            continue;
        }
        for argv in present {
            if io.run(argv, Some(CACHE_TIMEOUT_SECS), false).returncode == Some(0) {
                break;
            }
        }
    }
}

/// The wallpaper extension of one shell in `dest_base`: the old name (another
/// project's domain) is always removed, otherwise the wallpaper is applied
/// twice; our own extension missing from the payload is not an error.
fn install_extension<IO: InstallIo + ?Sized>(io: &IO, package_dir: &str, src_name: &str, dest_base: &str) -> Result<(), String> {
    io.remove_tree(&format!("{dest_base}/{}", inst::EXT_ID_LEGACY));
    let src = format!("{}/{src_name}/{}", package_dir.trim_end_matches('/'), inst::EXT_ID);
    if !io.is_dir(&src) {
        return Ok(());
    }
    let dst = format!("{dest_base}/{}", inst::EXT_ID);
    io.make_dirs(dest_base, None)?;
    io.remove_tree(&dst);
    io.copy_tree(&src, &dst, &payload_skip, &payload_mode)?;
    let schemas = format!("{dst}/schemas");
    if io.is_dir(&schemas) && io.which(GLIB_COMPILE_SCHEMAS) {
        io.run(&[GLIB_COMPILE_SCHEMAS.to_string(), schemas], Some(CACHE_TIMEOUT_SECS), false);
    }
    Ok(())
}

/// Install Rexon into the home folder — port of `RexonInstaller.install_user`.
///
/// No password. Order: program files are assembled in a temporary directory
/// next to the install directory → the old and new copies are swapped →
/// launch commands, desktop entries, icons, menu caches, wallpaper add-ons →
/// checking that Rexon is in place → profile folders. The temporary directory
/// (and the old copy in it) is removed on ANY outcome after it is created.
/// Cancelling is possible until the copies are swapped — then the previous
/// install is untouched.
pub fn install_for_user<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UserInstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    let lang = req.lang;
    let home = io.env("HOME").trim_end_matches('/').to_string();
    let xdg_data = io.env("XDG_DATA_HOME");
    let xdg_data = Some(xdg_data.as_str()).filter(|x| !x.is_empty());
    let target = inst::user_install_lib_dir(&home)
        .and_then(|_| InstallLayout::for_mode(InstallMode::User, &home, xdg_data));
    let Some(target) = target else {
        return finish(
            io,
            sink.as_ref(),
            lang,
            InstallMode::User,
            None,
            Err(Some(InstallFailure::StepFailed {
                step: InstallStep::CopyFiles,
                detail: trf("the home folder is not known (HOME=%1)", &[&format!("{home:?}")]),
            })),
        );
    };
    let paths = inst::install_paths(InstallMode::User, &home, xdg_data);
    let done = |res: Result<LaunchEntry, Option<InstallFailure>>| {
        finish(io, sink.as_ref(), lang, InstallMode::User, Some(target.bin_dir.as_str()), res.map(Some))
    };

    sink.cancellable(true);
    sink.progress(STAGE_PREPARE, &InstallStep::CopyFiles.progress_line(lang), Some(0.02));
    let parent = parent_dir(&target.lib_dir).to_string();
    let root = io.make_dirs(&parent, None).and_then(|()| io.temp_dir(USER_STAGE_PREFIX, Some(&parent)));
    let root = match root {
        Ok(r) => r,
        Err(detail) => {
            sink.cancellable(false);
            return done(Err(Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail })));
        }
    };
    let res = run_user(io, req, sink.as_ref(), cancel, &paths, &target, &StagePaths::new(&root));
    io.remove_tree(&root);
    sink.cancellable(false);
    done(res)
}

fn run_user<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UserInstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
    paths: &inst::InstallPaths,
    target: &InstallLayout,
    stage: &StagePaths,
) -> Result<LaunchEntry, Option<InstallFailure>> {
    use InstallStep::*;
    let lang = req.lang;
    let pay = &req.payload;
    let say = |step: InstallStep, fraction: f64| {
        sink.progress(STAGE_INSTALL, &step.progress_line(lang), Some(fraction));
    };
    let dir = |d: &Option<String>| d.clone().unwrap_or_default();
    let entry = installed_entry(req.entry_env.as_deref(), target, pay.main_window.is_some());

    if is_redirected(io, &target.lib_dir) {
        return Err(Some(InstallFailure::StepFailed {
            step: CopyFiles,
            detail: trf(
                "%1 is a link to another folder; the installation won't replace it",
                &[&target.lib_dir],
            ),
        }));
    }
    if cancel.is_cancelled() {
        return Err(None);
    }

    // ── Program files: assembled alongside, then swapped (fix 9) ────
    let staged = stage.layout();
    stage_program_files(io, pay, &staged)?;
    if cancel.is_cancelled() {
        return Err(None);
    }
    // From here on the home folder changes — cancel is unavailable.
    sink.cancellable(false);
    let previous = format!("{}/previous", stage.root);
    let had_previous = io.exists(&target.lib_dir);
    if had_previous {
        prep(CopyFiles, io.rename(&target.lib_dir, &previous))?;
    }
    if let Err(detail) = io.rename(&staged.lib_dir, &target.lib_dir) {
        if had_previous {
            let _ = io.rename(&previous, &target.lib_dir);
        }
        return Err(Some(InstallFailure::StepFailed { step: CopyFiles, detail }));
    }

    // ── Launch commands ─────────────────────────────────────────────
    say(Launcher, 0.4);
    prep(Launcher, io.make_dirs(&target.bin_dir, None))?;
    for (src, dst) in [
        (&pay.wallpaper_host, target.wallpaper_host()),
        (&pay.splash_overlay, target.splash_overlay()),
    ] {
        if let Some(src) = src {
            prep(Launcher, place_file(io, src, &dst, 0o755))?;
        }
    }
    let launcher = inst::user_launcher(&target.lib_dir);
    prep(Launcher, replace_text(io, &target.launcher(), &launcher, 0o755))?;

    // ── Desktop entries ─────────────────────────────────────────────
    say(MenuShortcuts, 0.5);
    let desktop_dir = dir(&paths.desktop_dir);
    prep(MenuShortcuts, io.make_dirs(&desktop_dir, None))?;
    for (name, text) in [
        (DESKTOP_FILE, inst::desktop_content_for(&entry)),
        (RECOVER_DESKTOP_FILE, inst::recover_desktop_content_for(&entry)),
    ] {
        prep(MenuShortcuts, replace_text(io, &format!("{desktop_dir}/{name}"), &text, 0o644))?;
    }

    // ── Icons ───────────────────────────────────────────────────────
    say(Icons, 0.6);
    let icon_root = dir(&paths.icon_root);
    let icons = inst::icon_targets(&payload_icons(io, &pay.icon_dir), &icon_root, &dir(&paths.pixmap_dir));
    if icons.is_empty() {
        sink.progress(de_progress::STAGE_WARNING, &inst::icons_missing_notice(lang), None);
    }
    for (src, dst) in &icons {
        prep(Icons, io.make_dirs(parent_dir(dst), None))?;
        prep(Icons, place_file(io, src, dst, 0o644))?;
    }

    // ── Menu and icon caches ────────────────────────────────────────
    say(MenuRefresh, 0.7);
    refresh_caches(io, Some(sink), lang, &desktop_dir, &icon_root);

    // ── GNOME and Cinnamon wallpaper add-ons ────────────────────────
    say(WallpaperAddons, 0.8);
    for (src_name, dest_base) in extension_targets(&dir(&paths.gnome_ext_dir), &dir(&paths.cinnamon_ext_dir)) {
        prep(WallpaperAddons, install_extension(io, &target.package_dir(), src_name, &dest_base))?;
    }

    // ── Check: Rexon is in place ────────────────────────────────────
    let in_place = inst::system_or_user_installed(paths, |p| io.exists(p), |p| io.is_dir(p))
        && match &entry {
            LaunchEntry::MainWindow(path) => io.is_executable(path),
            LaunchEntry::Python => true,
        };
    if !in_place {
        return Err(Some(InstallFailure::StepFailed {
            step: CopyFiles,
            detail: trf("%1 or %2 was not found after the installation", &[&target.launcher(), &target.lib_dir]),
        }));
    }

    // ── Profile folders ─────────────────────────────────────────────
    say(UserFolders, 0.95);
    make_profile_dirs(io);
    Ok(entry)
}

/// Portable mode — port of `_install_portable`: nothing to install, only the
/// profile folders are prepared. Summary — [`inst::portable_no_install_notice`].
pub fn install_portable<IO: InstallIo + ?Sized>(io: &IO, lang: Lang, sink: &dyn DeOpSink) -> InstallResult {
    sink.progress(STAGE_PREPARE, &InstallStep::UserFolders.progress_line(lang), Some(0.5));
    make_profile_dirs(io);
    finish(io, sink, lang, InstallMode::Portable, None, Ok(None))
}

/// There is no payload without the window binary: there would be nothing to
/// install. The check comes before the first side effect.
///
/// ⭐ Fix of 16.09.2026: previously such an install went through
/// "successfully" and pointed the desktop entry at the classic window
/// (`LaunchEntry::Python`). There is no Python in the payload any more, and
/// the `rexon` command now leads to the window binary itself
/// ([`inst::system_launcher`]) — an install without it would put into the
/// system a launcher pointing into the void.
fn payload_has_window(payload: &PayloadSource) -> Result<(), InstallFailure> {
    if payload.main_window.is_some() {
        return Ok(());
    }
    Err(InstallFailure::StepFailed {
        step: InstallStep::CopyFiles,
        detail: trf(
            "the Rexon window is missing from the package: %1 was not found next to it",
            &[&crate::window_menu::MAIN_WINDOW_BIN],
        ),
    })
}

/// Install Rexon in the chosen mode — the entry point for the wizard window.
///
/// Foolproofing (rule #13): a system install where the system folders are
/// immutable ([`inst::system_mode_blocked_reason_in`] — the same decision
/// `first_run_offer` gives the window as `system_blocked`) silently turns into
/// a home folder install BEFORE the first side effect; the reason goes to
/// progress and the summary (`fell_back`). Fix 10.
pub fn install<IO: InstallIo + ?Sized>(
    io: &IO,
    mode: InstallMode,
    req: &InstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> InstallResult {
    if mode != InstallMode::Portable {
        if let Err(failure) = payload_has_window(&req.payload) {
            return finish(io, sink.as_ref(), req.lang, mode, None, Err(Some(failure)));
        }
    }
    match mode {
        InstallMode::Portable => install_portable(io, req.lang, sink.as_ref()),
        InstallMode::User => install_for_user(io, &req.for_user(), sink, cancel),
        InstallMode::System => match inst::system_mode_blocked_reason_in(&req.distro_id, req.lang) {
            None => install_to_system(io, req, sink, cancel),
            Some(reason) => {
                sink.progress(STAGE_PREPARE, &reason, None);
                let mut result = install_for_user(io, &req.for_user(), sink, cancel);
                if let Some(WizardOutcome::Installed { fell_back, .. }) = &mut result.outcome {
                    *fell_back = Some(reason);
                }
                result
            }
        },
    }
}

// ============================================================================
// Uninstalling Rexon
// ============================================================================
//
// Port of `RexonUninstaller` (`rexon/installer.py`): distribution package →
// install for everyone → install for oneself → settings and backups.
// Decisions are in the core ([`inst::uninstall_system_paths`],
// [`inst::uninstall_user_paths`], [`inst::get_installed_files`]), here —
// execution through [`InstallIo`]. Fixes 15–19 are in the module header.

/// Ceiling of a "whose file is this" query to the package manager, s.
pub const OWNER_QUERY_TIMEOUT_SECS: u64 = 10;
/// Prefix of the uninstall root script's temporary folder.
pub const UNINSTALL_STAGE_PREFIX: &str = "rexon_uninstall_";
/// Uninstall stage.
pub const STAGE_UNINSTALL: &str = "app_uninstall";

/// The distribution package Rexon was installed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroPackage {
    pub family: PkgFamily,
    pub name: String,
}

/// Port of `_detect_distro_package`: ask the package manager for the owner of
/// our files. `None` — a manual, portable or developer copy.
///
/// `own_exe` — the path of our own executable: Rust has no `__file__`, and the
/// owner must be asked about the file that is actually installed in the system.
pub fn detect_distro_package<IO: SystemIo + ?Sized>(
    io: &IO,
    family: PkgFamily,
    own_exe: Option<&str>,
) -> Option<DistroPackage> {
    let owner = inst::distro_pkg_owner(family)?;
    let tool = owner.first()?.clone();
    if !io.which(&tool) {
        return None;
    }
    let mut candidates: Vec<String> = own_exe.and_then(|p| io.realpath(p)).into_iter().collect();
    for bin in inst::distro_candidate_bins() {
        if io.exists(bin) {
            candidates.extend(io.realpath(bin));
        }
    }
    candidates.into_iter().find_map(|path| {
        let mut argv = owner.clone();
        argv.push(path);
        let out = io.run(&argv, Some(OWNER_QUERY_TIMEOUT_SECS), true);
        if out.returncode != Some(0) {
            return None;
        }
        inst::parse_owner_output(&tool, &out.stdout).map(|name| DistroPackage { family, name })
    })
}

/// What to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallRequest {
    /// Distribution package ([`detect_distro_package`]).
    pub package: Option<DistroPackage>,
    /// An install for all users was found.
    pub system: bool,
    /// A home folder install was found.
    pub user: bool,
    /// «Uninstall but Keep Settings and Backups» (Wehrwolfmann's decision of
    /// 15.09.2026): settings (`$XDG_CONFIG_HOME/rexon`, which also holds the
    /// installed themes journal `ocs_installed.json`), desktop backups and the
    /// installed wallpapers, themes and add-ons (all in `$XDG_DATA_HOME/rexon`)
    /// are not touched, so that after reinstalling one can continue from the same
    /// place. Temporary files (`$XDG_CACHE_HOME/rexon`) are removed with any
    /// choice. The list — [`inst::user_data_roots`].
    pub keep_profile: bool,
    /// Language of the progress and summary strings.
    pub lang: Lang,
}

/// Home folder install paths; `None` — the home folder is not determined
/// (removing by a relative path is not allowed).
fn user_install_paths<IO: SystemIo + ?Sized>(io: &IO) -> Option<inst::InstallPaths> {
    let home = io.env("HOME").trim_end_matches('/').to_string();
    inst::user_install_lib_dir(&home)?;
    let xdg = io.env("XDG_DATA_HOME");
    Some(inst::install_paths(InstallMode::User, &home, Some(xdg.as_str()).filter(|x| !x.is_empty())))
}

fn installed_at<IO: SystemIo + ?Sized>(io: &IO, paths: &inst::InstallPaths) -> bool {
    inst::system_or_user_installed(paths, |p| io.exists(p), |p| io.is_dir(p))
}

impl UninstallRequest {
    /// What is installed on this machine — read only.
    pub fn detect<IO: SystemIo + ?Sized>(
        io: &IO,
        package: Option<DistroPackage>,
        keep_profile: bool,
        lang: Lang,
    ) -> Self {
        let system = installed_at(io, &inst::install_paths(InstallMode::System, "", None));
        let user = user_install_paths(io).is_some_and(|p| installed_at(io, &p));
        UninstallRequest { package, system, user, keep_profile, lang }
    }

    fn nothing_installed(&self) -> bool {
        self.package.is_none() && !self.system && !self.user
    }
}

/// Rexon user data on this machine: (category, path), existing ones only. The
/// root of installed content is expanded into what lies in it, except
/// backups; a root that is itself a link stays one line (the link itself
/// will be removed), and backups on the far side of such a link are not
/// named — that is someone else's folder already.
fn user_data_entries<IO: SystemIo + ?Sized>(io: &IO) -> Vec<(inst::UserDataKind, String)> {
    use inst::UserDataKind as K;
    let roots = inst::user_data_roots(
        &io.env("HOME"),
        &io.env("XDG_CONFIG_HOME"),
        &io.env("XDG_DATA_HOME"),
        &io.env("XDG_CACHE_HOME"),
    );
    let linked_data: Vec<String> = roots
        .iter()
        .filter(|(k, p)| *k == K::Content && is_redirected(io, p))
        .map(|(_, p)| format!("{p}/"))
        .collect();
    let mut out = Vec::new();
    for (kind, path) in roots {
        if !io.exists(&path) {
            continue;
        }
        match kind {
            K::Content if !is_redirected(io, &path) => {
                let mut names = io.list_dir(&path).unwrap_or_default();
                names.sort();
                out.extend(
                    names
                        .into_iter()
                        .filter(|n| n != inst::USER_DATA_BACKUPS_DIR)
                        .map(|n| (kind, format!("{path}/{n}"))),
                );
            }
            K::Backups if linked_data.iter().any(|root| path.starts_with(root)) => {}
            _ => out.push((kind, path)),
        }
    }
    out
}

/// The list for the «Uninstall Rexon» question dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallListing {
    /// Program files (and the distribution package) — removed with any choice.
    pub program: Vec<String>,
    /// The person's data by category; which of it is removed is decided by the
    /// choice ([`inst::UserDataKind::removed_when_keeping`]).
    pub user_data: Vec<(inst::UserDataKind, String)>,
    /// Rexon's protective files in the config folder — kept with EITHER choice
    /// ([`crate::appdefaults::protective_files`]); named in the window so the
    /// person knows they stay and why.
    pub kept_fixes: Vec<String>,
}

impl UninstallListing {
    /// Everything removed with this choice — the program, then the data.
    pub fn removed(&self, keep_profile: bool) -> Vec<String> {
        let data = self
            .user_data
            .iter()
            .filter(|(k, _)| !keep_profile || k.removed_when_keeping())
            .map(|(_, p)| p.clone());
        self.program.iter().cloned().chain(data).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.program.is_empty() && self.user_data.is_empty()
    }
}

/// Everything [`uninstall`] will remove — for the question dialog. Read only.
/// Links in place of the program, settings and backups are not named: they
/// won't be removed. A link in place of temporary files or installed content
/// is named: the link itself will be removed. Nothing installed — the list is
/// empty (uninstall then touches nothing).
pub fn uninstall_files<IO: SystemIo + ?Sized>(io: &IO, req: &UninstallRequest) -> UninstallListing {
    if req.nothing_installed() {
        return UninstallListing::default();
    }
    let sys = inst::install_paths(InstallMode::System, "", None);
    let usr = user_install_paths(io).filter(|_| req.user);
    // Settings and backups go as their own categories, not as program paths.
    let program = inst::get_installed_files(
        req.package.as_ref().map(|p| p.name.as_str()),
        req.system.then_some(&sys),
        usr.as_ref(),
        "",
        "",
        |p| !p.is_empty() && io.exists(p) && !is_redirected(io, p),
    );
    let user_data = user_data_entries(io)
        .into_iter()
        .filter(|(kind, p)| {
            !matches!(kind, inst::UserDataKind::Settings | inst::UserDataKind::Backups)
                || !is_redirected(io, p)
        })
        .collect();
    UninstallListing { program, user_data, kept_fixes: kept_protective_files(io) }
}

/// [`crate::appdefaults::protective_files`] on this machine (empty if the home
/// folder isn't absolute).
fn kept_protective_files<IO: SystemIo + ?Sized>(io: &IO) -> Vec<String> {
    let home = io.env("HOME");
    if !home.starts_with('/') {
        return Vec::new();
    }
    let xdg = io.env("XDG_CONFIG_HOME");
    let cfg = crate::utils::config_home(Some(xdg.as_str()).filter(|x| x.starts_with('/')), home.trim_end_matches('/'));
    crate::appdefaults::protective_files(&cfg, &|p| io.read_text(p))
}

/// How the uninstall ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallOutcome {
    /// Removed; `kept_links` — links to foreign folders that were left alone.
    Removed { kept_links: Vec<String> },
    /// Nothing to remove — nothing touched.
    NotInstalled,
    /// Cancelled before the password prompt — nothing touched.
    Cancelled,
    /// A package Rexon doesn't know how to remove — nothing touched.
    PackageUnsupported { name: String },
    Failed(InstallFailure),
}

/// Outcome and the summary line for the person ([`inst::uninstall_done_line`]
/// and neighbours).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallResult {
    pub outcome: UninstallOutcome,
    pub message: String,
}

impl UninstallResult {
    pub fn ok(&self) -> bool {
        matches!(self.outcome, UninstallOutcome::Removed { .. })
    }
}

/// The uninstall plan is fit for the executor: writing the script, running the
/// removal under a password, cleanup. A foreign step is a refusal, not a quiet "done".
fn check_uninstall_plan(steps: &[PlanStep]) -> bool {
    steps.iter().all(|s| {
        matches!(
            s,
            PlanStep::WriteFile { .. }
                | PlanStep::RemoveFiles { .. }
                | PlanStep::Run { id: RunStepId::AppUninstall, .. }
        )
    })
}

/// (to remove, links to foreign folders). A link is neither removed nor
/// traversed: fix 16.
fn split_links<IO: SystemIo + ?Sized>(io: &IO, paths: Vec<String>) -> (Vec<String>, Vec<String>) {
    paths.into_iter().partition(|p| !is_redirected(io, p))
}

/// Remove a path: a whole directory or a file (a dangling link — as a file).
fn remove_path<IO: InstallIo + ?Sized>(io: &IO, path: &str) {
    if io.is_dir(path) {
        io.remove_tree(path)
    } else {
        io.remove_file(path)
    }
}

/// An uninstall failure. The step is not named to the person on an uninstall
/// refusal ([`inst::uninstall_failure_line`]), the field is only needed by the type.
fn removal_failed(detail: impl Into<String>) -> UninstallOutcome {
    UninstallOutcome::Failed(InstallFailure::StepFailed {
        step: InstallStep::SystemInstall,
        detail: detail.into(),
    })
}

/// Uninstall Rexon — the entry point of the window's «Uninstall Rexon» menu.
///
/// Order: refusals before side effects (nothing to remove, a package with no
/// known way to remove it, cancel) → the package via its manager → the
/// install for everyone with one root script under a password → the install
/// for oneself without a password → temporary files (with any choice) →
/// settings, backups and installed wallpapers, themes and add-ons (with
/// «keep» none of these is touched).
/// A failure stops the uninstall: the profile is removed last and only after
/// everything else succeeded. Cancelling is possible until the first password prompt.
pub fn uninstall<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UninstallRequest,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> UninstallResult {
    let lang = req.lang;
    let sink = sink.as_ref();
    let mut unlinked: Vec<String> = Vec::new();
    let outcome = if req.nothing_installed() {
        UninstallOutcome::NotInstalled
    } else {
        sink.cancellable(true);
        let res = run_uninstall(io, req, sink, cancel);
        sink.cancellable(false);
        match res {
            Ok(removal) => {
                unlinked = removal.unlinked;
                UninstallOutcome::Removed { kept_links: removal.kept }
            }
            Err(outcome) => outcome,
        }
    };
    let (stage, message) = match &outcome {
        UninstallOutcome::Removed { kept_links } => {
            let mut message = inst::uninstall_done_line(req.keep_profile, lang);
            for link in kept_links {
                message.push_str("\n\n");
                message.push_str(&inst::uninstall_kept_link_note(link, lang));
            }
            for link in &unlinked {
                message.push_str("\n\n");
                message.push_str(&inst::uninstall_removed_link_note(link, lang));
            }
            (STAGE_DONE, message)
        }
        UninstallOutcome::NotInstalled => (de_progress::STAGE_WARNING, inst::uninstall_nothing_line(lang)),
        UninstallOutcome::Cancelled => (
            crate::system_de_exec::STATUS_CANCELLED,
            crate::system_de_exec::cancelled_message(),
        ),
        UninstallOutcome::PackageUnsupported { name } => {
            (de_progress::STAGE_ERROR, inst::uninstall_package_unsupported_line(name, lang))
        }
        UninstallOutcome::Failed(f) => (de_progress::STAGE_ERROR, inst::uninstall_failure_line(f, lang)),
    };
    let fraction = matches!(outcome, UninstallOutcome::Removed { .. }).then_some(1.0);
    sink.progress(stage, &message, fraction);
    UninstallResult { outcome, message }
}

fn run_uninstall<IO: InstallIo + ?Sized>(
    io: &IO,
    req: &UninstallRequest,
    sink: &dyn DeOpSink,
    cancel: &CancelToken,
) -> Result<Removal, UninstallOutcome> {
    let lang = req.lang;
    let say = |part: inst::UninstallPart, fraction: f64| {
        sink.progress(STAGE_UNINSTALL, &part.progress_line(lang), Some(fraction));
    };
    // ── Refusals before the first side effect ───────────────────────
    let package_cmd = match &req.package {
        Some(pkg) => match inst::distro_pkg_remove(pkg.family) {
            Some(remove) => Some(inst::pkg_remove_cmd(&remove, &pkg.name)),
            None => return Err(UninstallOutcome::PackageUnsupported { name: pkg.name.clone() }),
        },
        None => None,
    };
    if cancel.is_cancelled() {
        return Err(UninstallOutcome::Cancelled);
    }
    let mut kept = Vec::new();

    // ── 1. Distribution package — only via its manager ──────────────
    if let Some(cmd) = package_cmd {
        sink.cancellable(false);
        say(inst::UninstallPart::Package, 0.1);
        let out = io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        classify_script_run(&out, cmd.timeout_secs.unwrap_or(0)).map_err(UninstallOutcome::Failed)?;
    }

    // ── 2. Install for everyone: one root script under a password ───
    if req.system {
        // Cancel is still possible only if the password hasn't been asked.
        let cancel = req.package.is_none().then_some(cancel);
        uninstall_system(io, lang, sink, cancel, &mut kept)?;
    }
    sink.cancellable(false);

    // ── 3. Home folder install: no password ─────────────────────────
    if let Some(usr) = user_install_paths(io).filter(|_| req.user) {
        say(inst::UninstallPart::User, 0.7);
        let (remove, links) = split_links(io, inst::uninstall_user_paths(&usr, |p| io.exists(p)));
        kept.extend(links);
        for p in &remove {
            remove_path(io, p);
        }
        let dir = |d: &Option<String>| d.clone().unwrap_or_default();
        refresh_caches(io, None, lang, &dir(&usr.desktop_dir), &dir(&usr.icon_root));
    }

    // ── 3½. Login and Plasma session guards — with ANY choice ───────
    // The program is gone: restore the rolled-back appearance and remove the unit
    // now, otherwise there will be no one left to restore it.
    release_session_guard(io);

    // ── 4. Temporary files — with ANY choice ────────────────────────
    let data = user_data_entries(io);
    let mut unlinked = Vec::new();
    let (always, on_full): (Vec<_>, Vec<_>) = data.into_iter().partition(|(k, _)| k.removed_when_keeping());
    if !always.is_empty() {
        say(inst::UninstallPart::Cache, 0.8);
        for (_, p) in &always {
            remove_own_data(io, p, &mut unlinked);
        }
    }

    // ── 5. Settings, backups, installed wallpapers and themes ────────
    // Last, and only with «Remove Everything». «Keep» leaves all of this: a
    // person who reinstalls Rexon will continue from the same place.
    if !req.keep_profile && !on_full.is_empty() {
        say(inst::UninstallPart::Profile, 0.9);
        for (kind, p) in &on_full {
            match kind {
                // As before (fix 16): a link is neither removed nor traversed.
                inst::UserDataKind::Settings | inst::UserDataKind::Backups => {
                    if is_redirected(io, p) {
                        kept.push(p.clone());
                    } else {
                        remove_path(io, p);
                    }
                }
                _ => remove_own_data(io, p, &mut unlinked),
            }
        }
        // An emptied data root — too. A non-empty one (a link to the backups
        // remained) is not touched.
        let home = io.env("HOME");
        for (_, root) in inst::user_data_roots(&home, "", &io.env("XDG_DATA_HOME"), "")
            .into_iter()
            .filter(|(k, _)| *k == inst::UserDataKind::Content)
        {
            if io.is_dir(&root) && !is_redirected(io, &root) && io.list_dir(&root).is_some_and(|l| l.is_empty()) {
                io.remove_tree(&root);
            }
        }
    }
    Ok(Removal { kept, unlinked })
}

/// Uninstalling Rexon during an AeroThemePlasma rollback
/// ([`crate::session_guard::release_for_uninstall`]): KWin effects — right
/// away, the layout — with a one-shot script at the next login; the session
/// guard unit is removed. No root; a failure goes only to the log: it is no
/// obstacle to removing the program.
fn release_session_guard<IO: InstallIo + ?Sized>(io: &IO) {
    let home = io.env("HOME");
    if !home.starts_with('/') {
        return;
    }
    let abs = |v: String| Some(v).filter(|v| v.starts_with('/'));
    let config = crate::utils::config_home(abs(io.env("XDG_CONFIG_HOME")).as_deref(), &home);
    let state = crate::utils::state_home(abs(io.env("XDG_STATE_HOME")).as_deref(), &home);
    let paths = crate::session_guard::GuardPaths::new(&config, std::path::Path::new(&state).join("rexon/session-guard"));
    match crate::session_guard::release_for_uninstall(&paths) {
        Ok(done) => {
            if done.restored.had_saved {
                crate::log_info!("session-guard", "удаление Rexon: возвращено {done:?}");
            }
            if !done.restored.effects.is_empty() && io.which("dbus-send") {
                io.run(&crate::session_guard::reconfigure_kwin_argv(), Some(5), true);
            }
        }
        Err(e) => crate::log_warn!("session-guard", "удаление Rexon: {e}"),
    }
    let unit = crate::session_watch::unit_path(&config);
    if io.exists(&unit) {
        let has_systemctl = io.which("systemctl");
        for args in crate::session_watch::uninstall_systemctl() {
            if args.first().map(String::as_str) == Some("daemon-reload") {
                io.remove_file(&unit);
            }
            if has_systemctl {
                let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
                argv.extend(args);
                io.run(&argv, Some(30), true);
            }
        }
    }
    // The notification daemon choice (`notifications_slot`): without Rexon nobody
    // will update it. We remove only OUR file (the marker in the first line); a
    // foreign one put there by the person stays. The bus only rereads its configuration.
    let slot = crate::notifications_slot::SlotPaths::from_env(&home, &io.env("XDG_DATA_HOME")).user_file;
    let slot = slot.to_string_lossy();
    if io.read_text(&slot).is_some_and(|t| crate::notifications_slot::is_ours(&t)) {
        io.remove_file(&slot);
        if io.which("busctl") {
            io.run(&crate::appdefaults::busctl_reload_cmd(), Some(10), true);
        }
    }
    // Per-desktop default applications (`defaults::protect_de_mimeapps`): without
    // Rexon nobody follows the person's choices into them any more, and a
    // `<desktop>-mimeapps.list` is read BEFORE the person's own `mimeapps.list`.
    // Only OUR lists go (the marker); the person's stay.
    for name in io.list_dir(&config).unwrap_or_default() {
        if !name.ends_with("-mimeapps.list") {
            continue;
        }
        let path = format!("{config}/{name}");
        if io.read_text(&path).is_some_and(|t| crate::defaults::is_rexon_mimeapps(&t)) {
            io.remove_file(&path);
            crate::log_info!("install", "uninstall: removed our {path}");
        }
    }
    // Screen saver launchers hidden in foreign desktops (`screensaver_guard`):
    // only OUR copies (the marker line) go; the person's own stay. The system
    // launchers show again as their packages ship them.
    let launchers = crate::screensaver_guard::GuardPaths::from_env(
        &home,
        &io.env("XDG_DATA_HOME"),
        &io.env("XDG_CONFIG_HOME"),
        &io.env("XDG_DATA_DIRS"),
    )
    .user_dir;
    for l in crate::screensaver_guard::LAUNCHERS {
        let copy = launchers.join(l.file);
        let copy = copy.to_string_lossy();
        if io.read_text(&copy).is_some_and(|t| crate::screensaver_guard::is_ours(&t)) {
            io.remove_file(&copy);
        }
    }
    // Light and dark on a schedule: the timer, the service and autostart would
    // wake up nothing without Rexon. The Plasma setting (its own «Automatically»)
    // is left alone — it is Plasma's own setting.
    let dn = crate::day_night::Paths { config_home: config, state_home: state };
    let present: Vec<String> = crate::day_night::uninstall_paths(&dn).into_iter().filter(|p| io.exists(p)).collect();
    if !present.is_empty() {
        let has_systemctl = io.which("systemctl");
        let sc = |args: &[&str]| {
            if has_systemctl {
                let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
                argv.extend(args.iter().map(|a| a.to_string()));
                io.run(&argv, Some(30), true);
            }
        };
        sc(&["disable", "--now", crate::day_night::TIMER_UNIT]);
        for p in &present {
            io.remove_file(p);
        }
        sc(&["daemon-reload"]);
    }
}

/// What was left and what was removed as a link — for the summary.
struct Removal {
    /// Links in place of the program, settings and backups: left untouched.
    kept: Vec<String>,
    /// Links in place of temporary files and installed content: the link itself was removed.
    unlinked: Vec<String>,
}

/// Temporary files and installed content. A link to another folder is removed
/// ITSELF, we don't follow it: what it points to is not ours. The rest — in
/// full; links inside the directory are removed as links too (tree removal
/// doesn't follow them).
fn remove_own_data<IO: InstallIo + ?Sized>(io: &IO, path: &str, unlinked: &mut Vec<String>) {
    if is_redirected(io, path) {
        io.remove_file(path);
        unlinked.push(path.to_string());
    } else {
        remove_path(io, path);
    }
}

/// Install for everyone: the script in a temporary folder → password →
/// cleanup of the folder on ANY outcome (fix 18). Links from the list don't get into the script.
fn uninstall_system<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    sink: &dyn DeOpSink,
    cancel: Option<&CancelToken>,
    kept: &mut Vec<String>,
) -> Result<(), UninstallOutcome> {
    let sys = inst::install_paths(InstallMode::System, "", None);
    let (paths, links) = split_links(io, inst::uninstall_system_paths(&sys));
    let root = io.temp_dir(UNINSTALL_STAGE_PREFIX, None).map_err(removal_failed)?;
    let res = run_system_script(io, lang, sink, cancel, &sys, &paths, &root);
    io.remove_tree(&root);
    res?;
    kept.extend(links);
    Ok(())
}

fn run_system_script<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    sink: &dyn DeOpSink,
    cancel: Option<&CancelToken>,
    sys: &inst::InstallPaths,
    paths: &[String],
    root: &str,
) -> Result<(), UninstallOutcome> {
    let foreign_step = || removal_failed(tr("the removal plan contains an unknown step"));
    let script = format!("{}/uninstall.sh", root.trim_end_matches('/'));
    let steps = inst::uninstall_system_plan_for(sys, paths, &script);
    if !check_uninstall_plan(&steps) {
        return Err(foreign_step());
    }
    let mut run_out: Option<(RunOut, u64)> = None;
    for step in &steps {
        match step {
            PlanStep::WriteFile { path, content, mode } => {
                io.write_file_mode(path, content, *mode).map_err(removal_failed)?;
            }
            PlanStep::Run { cmd, .. } => {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    return Err(UninstallOutcome::Cancelled);
                }
                sink.cancellable(false);
                sink.progress(STAGE_UNINSTALL, &inst::UninstallPart::System.progress_line(lang), Some(0.4));
                run_out = Some((io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c), cmd.timeout_secs.unwrap_or(0)));
            }
            // Cleanup is done by `uninstall_system` — the whole folder, on any outcome.
            PlanStep::RemoveFiles { .. } => {}
            _ => return Err(foreign_step()),
        }
    }
    let (out, secs) = run_out.unwrap_or_else(|| (RunOut::missing(), 0));
    classify_script_run(&out, secs).map_err(UninstallOutcome::Failed)
}

/// A progress receiver without a window: lines go to the Rexon log (`applog`). Used by
/// the window's «Uninstall Rexon» menu — it has no progress window, the result arrives as a signal.
pub struct LogSink;

impl DeOpSink for LogSink {
    fn progress(&self, stage: &str, message: &str, _fraction: Option<f64>) {
        crate::log_info!("uninstall", "[{stage}] {message}");
    }
    fn cancellable(&self, _allowed: bool) {}
}

// ============================================================================
// Live executor
// ============================================================================

/// [`InstallIo`] on top of the live machine — what the product uses.
pub struct LiveInstallIo {
    de: LiveDeOpsIo,
}

impl LiveInstallIo {
    pub fn new() -> Self {
        LiveInstallIo { de: LiveDeOpsIo::new() }
    }
}

impl Default for LiveInstallIo {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemIo for LiveInstallIo {
    fn exists(&self, path: &str) -> bool {
        self.de.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.de.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.de.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.de.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.de.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.de.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.de.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.de.which(name)
    }
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        self.de.run(argv, timeout_secs, lang_c)
    }
    fn env(&self, name: &str) -> String {
        self.de.env(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.de.which_path(name)
    }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.de.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.de.copy_file(src, dst)
    }
}

fn set_mode(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn walk_copy(
    src: &std::path::Path,
    dst: &std::path::Path,
    skip: &dyn Fn(&str) -> bool,
    mode: &dyn Fn(bool, bool) -> u32,
    count: &mut usize,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    set_mode(dst, mode(true, false))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if skip(&name) {
            continue;
        }
        let path = entry.path();
        let target = dst.join(&name);
        let Ok(link) = std::fs::symlink_metadata(&path) else { continue };
        // A dangling symlink — skipped (like `ignore_dangling_symlinks`).
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            // A symlink to a directory — skipped: a loop or a path outside the payload.
            if link.file_type().is_symlink() {
                continue;
            }
            walk_copy(&path, &target, skip, mode, count)?;
        } else if meta.is_file() {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
            set_mode(&target, mode(false, meta.permissions().mode() & 0o111 != 0))?;
            *count += 1;
        }
    }
    Ok(())
}

impl InstallIo for LiveInstallIo {
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        DeOpsIo::temp_dir(&self.de, prefix, parent)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
        std::fs::rename(src, dst).map_err(|e| format!("{src} → {dst}: {e}"))
    }

    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
        let target = std::path::Path::new(path);
        let missing: Vec<&std::path::Path> = target
            .ancestors()
            .take_while(|p| !p.as_os_str().is_empty() && !p.exists())
            .collect();
        std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
        if let Some(mode) = mode {
            for dir in missing.iter().rev() {
                set_mode(dir, mode)?;
            }
        }
        Ok(())
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        DeOpsIo::write_file_mode(&self.de, path, content, mode)
    }

    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
        std::fs::copy(src, dst).map_err(|e| format!("{src}: {e}"))?;
        set_mode(std::path::Path::new(dst), mode)
    }

    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        skip: &dyn Fn(&str) -> bool,
        mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String> {
        let mut count = 0;
        walk_copy(std::path::Path::new(src), std::path::Path::new(dst), skip, mode, &mut count)?;
        Ok(count)
    }

    fn remove_file(&self, path: &str) {
        DeOpsIo::remove_file(&self.de, path)
    }

    fn remove_tree(&self, path: &str) {
        DeOpsIo::remove_tree(&self.de, path)
    }
}

// ============================================================================
// Tests — ONLY the substitute executor
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        TempDir,
        MakeDirs(String, Option<u32>),
        Write(String, String, u32),
        CopyFile(String, String, u32),
        CopyTree(String, String),
        TreeItem(String, u32),
        Run(Vec<String>, Option<u64>),
        Rename(String, String),
        RemoveFile(String),
        RemoveTree(String),
    }

    const STAGE: &str = "/tmp/rexon_install_T";
    const RU: Lang = Lang::Ru;

    type Reply = Box<dyn Fn(&[String]) -> RunOut>;

    struct FakeIo {
        calls: RefCell<Vec<Call>>,
        files: HashSet<String>,
        dirs: HashSet<String>,
        /// Tree contents: (relative path, directory, executable).
        trees: HashMap<String, Vec<(String, bool, bool)>>,
        env: HashMap<String, String>,
        reply: Reply,
        /// The script "put" files — the target is visible after the run.
        lands: bool,
        landed: Cell<bool>,
        fail_temp_dir: bool,
        fail_copy_tree: bool,
        /// Links: path → where it leads (`realpath`).
        links: HashMap<String, String>,
        /// Programs that "exist" (`which`).
        tools: HashSet<String>,
        /// File texts for `read_text` (and names for `list_dir`).
        texts: HashMap<String, String>,
    }

    fn out(code: i32, stdout: &str, stderr: &str) -> RunOut {
        RunOut {
            outcome: if code == 0 { RunOutcome::Ok } else { RunOutcome::Failed },
            returncode: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    fn marks(ids: &[&str]) -> String {
        ids.iter().map(|s| format!("{STEP_MARK}{s}\n")).collect()
    }

    fn all_marks() -> String {
        marks(&["deps", "files", "launcher", "menu", "icons", "helpers", "caches", "extensions"])
    }

    impl FakeIo {
        fn new(pkexec: impl Fn() -> RunOut + 'static) -> Self {
            let files: HashSet<String> = [
                "/src/rexon-main-window",
                "/src/rexon-theme-shot",
                "/src/bin/rexon-wallpaper-host",
                "/src/bin/rexon-splash-overlay",
                "/src/bin/rexon-priv-helper",
                "/src/qml_modules/waywallen/control/librexon_qt_filter.so",
                "/src/rexon_icon_16.png",
                "/src/rexon_icon_128.png",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let mut trees = HashMap::new();
            trees.insert(
                "/src/rexon".to_string(),
                vec![
                    ("app.py".to_string(), false, false),
                    ("themes.json".to_string(), false, false),
                    ("tools".to_string(), true, false),
                    ("tools/run.sh".to_string(), false, true),
                    ("__pycache__".to_string(), true, false),
                    ("__pycache__/app.cpython.pyc".to_string(), false, false),
                    (".hidden.json".to_string(), false, false),
                    ("gnome-extension".to_string(), true, false),
                ],
            );
            trees.insert(
                "/src/qml_modules".to_string(),
                vec![("Qcm/Material/libqml_materialplugin.so".to_string(), false, true)],
            );
            let env: HashMap<String, String> = [
                ("HOME".to_string(), "/scratch/home".to_string()),
                ("PATH".to_string(), "/usr/local/bin:/usr/bin".to_string()),
            ]
            .into_iter()
            .collect();
            FakeIo {
                calls: RefCell::new(Vec::new()),
                files,
                dirs: ["/src/qml_modules".to_string()].into_iter().collect(),
                trees,
                env,
                reply: Box::new(move |argv: &[String]| {
                    if argv.first().map(String::as_str) == Some("pkexec") {
                        pkexec()
                    } else {
                        out(0, "", "")
                    }
                }),
                lands: true,
                landed: Cell::new(false),
                fail_temp_dir: false,
                fail_copy_tree: false,
                links: HashMap::new(),
                tools: HashSet::new(),
                texts: HashMap::new(),
            }
        }

        fn ok() -> Self {
            Self::new(|| out(0, &all_marks(), ""))
        }

        fn log(&self, c: Call) {
            self.calls.borrow_mut().push(c);
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn runs(&self) -> Vec<(Vec<String>, Option<u64>)> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Run(a, t) if a[0] == "pkexec" => Some((a, t)),
                    _ => None,
                })
                .collect()
        }

        fn written(&self, path: &str) -> Option<(String, u32)> {
            self.calls().into_iter().find_map(|c| match c {
                Call::Write(p, text, mode) if p == path => Some((text, mode)),
                _ => None,
            })
        }

        fn target_visible(&self, path: &str) -> bool {
            self.landed.get()
                && (path.starts_with("/usr/local/") || path.starts_with("/usr/share/applications/"))
        }
    }

    impl SystemIo for FakeIo {
        fn exists(&self, path: &str) -> bool {
            self.files.contains(path) || self.dirs.contains(path) || self.target_visible(path)
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(path) || self.target_visible(path)
        }
        fn is_executable(&self, path: &str) -> bool {
            self.target_visible(path)
        }
        fn realpath(&self, path: &str) -> Option<String> {
            Some(self.links.get(path).cloned().unwrap_or_else(|| path.to_string()))
        }
        fn read_text(&self, path: &str) -> Option<String> {
            self.texts.get(path).cloned()
        }
        fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
            let prefix = format!("{dir}/");
            let names: Vec<String> = self
                .texts
                .keys()
                .filter_map(|p| p.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_string)
                .collect();
            (!names.is_empty()).then_some(names)
        }
        fn which(&self, name: &str) -> bool {
            self.tools.contains(name)
        }
        fn run(&self, argv: &[String], timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
            self.log(Call::Run(argv.to_vec(), timeout_secs));
            let r = (self.reply)(argv);
            if argv[0] == "pkexec" && r.returncode == Some(0) && self.lands {
                self.landed.set(true);
            }
            r
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, _content: &str) -> Result<(), String> {
            panic!("write_text без прав исполнитель установки не зовёт: {path}")
        }
        fn copy_file(&self, src: &str, _dst: &str) -> Result<(), String> {
            panic!("copy_file без прав исполнитель установки не зовёт: {src}")
        }
    }

    impl InstallIo for FakeIo {
        fn temp_dir(&self, _prefix: &str, parent: Option<&str>) -> Result<String, String> {
            self.log(Call::TempDir);
            if self.fail_temp_dir {
                Err("нет места".into())
            } else {
                // A directory inside `parent`, as with the live one; without it — the previous one.
                Ok(parent.map_or_else(|| STAGE.to_string(), |p| format!("{p}/{}", basename(STAGE))))
            }
        }
        fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
            self.log(Call::Rename(src.to_string(), dst.to_string()));
            Ok(())
        }
        fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
            self.log(Call::MakeDirs(path.to_string(), mode));
            Ok(())
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.log(Call::Write(path.to_string(), content.to_string(), mode));
            Ok(())
        }
        fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
            self.log(Call::CopyFile(src.to_string(), dst.to_string(), mode));
            Ok(())
        }
        fn copy_tree(
            &self,
            src: &str,
            dst: &str,
            skip: &dyn Fn(&str) -> bool,
            mode: &dyn Fn(bool, bool) -> u32,
        ) -> Result<usize, String> {
            self.log(Call::CopyTree(src.to_string(), dst.to_string()));
            if self.fail_copy_tree {
                return Err("диск переполнен".into());
            }
            let mut n = 0;
            for (rel, is_dir, exec) in self.trees.get(src).cloned().unwrap_or_default() {
                if rel.split('/').any(skip) {
                    continue;
                }
                self.log(Call::TreeItem(format!("{dst}/{rel}"), mode(is_dir, exec)));
                n += usize::from(!is_dir);
            }
            Ok(n)
        }
        fn remove_file(&self, path: &str) {
            self.log(Call::RemoveFile(path.to_string()));
        }
        fn remove_tree(&self, path: &str) {
            self.log(Call::RemoveTree(path.to_string()));
        }
    }

    #[derive(Default)]
    struct RecSink {
        lines: Mutex<Vec<(String, String)>>,
        cancellable: Mutex<Vec<bool>>,
    }

    impl RecSink {
        fn has(&self, message: &str) -> bool {
            self.lines.lock().unwrap().iter().any(|(_, m)| m == message)
        }
    }

    impl DeOpSink for RecSink {
        fn progress(&self, stage: &str, message: &str, _fraction: Option<f64>) {
            self.lines.lock().unwrap().push((stage.to_string(), message.to_string()));
        }
        fn cancellable(&self, allowed: bool) {
            self.cancellable.lock().unwrap().push(allowed);
        }
    }

    fn payload() -> PayloadSource {
        PayloadSource {
            package_dir: "/src/rexon".into(),
            icon_dir: "/src".into(),
            main_window: Some("/src/rexon-main-window".into()),
            wallpaper_host: Some("/src/bin/rexon-wallpaper-host".into()),
            splash_overlay: Some("/src/bin/rexon-splash-overlay".into()),
            qml_modules: Some("/src/qml_modules".into()),
            filter_lib: Some("/src/qml_modules/waywallen/control/librexon_qt_filter.so".into()),
            priv_helper: Some("/src/bin/rexon-priv-helper".into()),
        }
    }

    fn request() -> InstallRequest {
        InstallRequest::new(payload(), PkgFamily::Arch, "cachyos", RU)
    }

    fn install(io: &FakeIo, req: &InstallRequest) -> (InstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = install_to_system(io, req, sink.clone(), &CancelToken::new());
        (r, sink)
    }

    fn script_of(io: &FakeIo) -> String {
        io.written(&StagePaths::new(STAGE).script()).expect("скрипт записан").0
    }

    /// Cleanup: the temporary directory is removed AFTER all side effects in it.
    fn cleaned_last(io: &FakeIo) {
        let calls = io.calls();
        let remove = calls.iter().rposition(|c| *c == Call::RemoveTree(STAGE.into()));
        let last_side = calls.iter().rposition(|c| match c {
            Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) => {
                p.starts_with(STAGE)
            }
            Call::Run(a, _) => a[0] == "pkexec",
            _ => false,
        });
        assert!(remove.is_some(), "временный каталог убран: {calls:#?}");
        assert!(remove > last_side, "уборка — после всех действий во временном каталоге");
    }

    fn failure(r: &InstallResult) -> InstallFailure {
        r.failure().cloned().unwrap_or_else(|| panic!("ждали неудачу: {r:?}"))
    }

    // ── Success ─────────────────────────────────────────────────────

    #[test]
    fn success_runs_script_once_under_password_and_cleans_up() {
        let io = FakeIo::ok();
        let req = request();
        let (r, sink) = install(&io, &req);
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.message, inst::install_done_line(InstallMode::System, RU));
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed {
                mode: InstallMode::System,
                fell_back: None,
                command_dir_not_in_path: None,
            })
        );

        let script = StagePaths::new(STAGE).script();
        assert_eq!(
            io.runs(),
            vec![(vec!["pkexec".to_string(), "bash".into(), script.clone()], Some(600))],
            "один запуск под паролем; починка 3: таймаут 600 с"
        );

        // The written script is exactly the core's text for these parameters.
        let params = system_script_params(&io, &req, &StagePaths::new(STAGE));
        let (text, mode) = io.written(&script).unwrap();
        assert_eq!(text, install_script(&params));
        assert_eq!(mode, 0o700);

        // Everything written and copied is in the temporary directory or in the
        // substitute HOME: the executor itself touches neither /usr nor /etc.
        for c in io.calls() {
            let dst = match &c {
                Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) => p,
                _ => continue,
            };
            assert!(
                dst.starts_with(STAGE) || dst.starts_with("/scratch/home/"),
                "запись вне временного каталога: {c:?}"
            );
        }
        cleaned_last(&io);

        assert!(sink.has(&format!("✅ {}", InstallStep::CopyFiles.progress_line(RU).trim_end_matches('…'))));
        let lines = sink.lines.lock().unwrap().clone();
        assert_eq!(lines.last().unwrap().0, STAGE_DONE);
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));

        let calls = io.calls();
        assert!(calls.contains(&Call::MakeDirs("/scratch/home/.config/rexon".into(), None)));
        assert!(calls.contains(&Call::MakeDirs("/scratch/home/.local/share/rexon/backups".into(), None)));
    }

    #[test]
    fn command_dir_outside_path_is_reported() {
        let mut io = FakeIo::ok();
        io.env.insert("PATH".into(), "/usr/bin".into());
        let (r, _) = install(&io, &request());
        assert!(matches!(
            r.outcome,
            Some(WizardOutcome::Installed { command_dir_not_in_path: Some(ref d), .. }) if d == "/usr/local/bin"
        ));
    }

    // ── Password refusal and step failure (fix 1) ───────────────────

    #[test]
    fn closed_password_window_is_password_denied_not_a_failed_install() {
        let io = FakeIo::new(|| out(126, "", "Error executing command as another user: Request dismissed"));
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordDenied);
        assert_eq!(r.message, InstallFailure::PasswordDenied.log_line(RU));
        assert!(!r.message.contains("pkexec") && !r.message.contains("отменена пользователем"));
        cleaned_last(&io);
    }

    #[test]
    fn denied_password_without_started_script_is_password_denied() {
        let io = FakeIo::new(|| out(127, "", "Error executing command as another user: Not authorized"));
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordDenied);
        cleaned_last(&io);
    }

    #[test]
    fn failed_dependency_step_is_a_step_failure_not_a_cancel() {
        let io = FakeIo::new(|| {
            out(
                1,
                &marks(&["deps"]),
                &format!("error: target not found: layer-shell-qt\n{FAIL_MARK}deps\n"),
            )
        });
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (r, sink) = install(&io, &request());
        let f = failure(&r);
        let InstallFailure::StepFailed { step, detail } = &f else { panic!("{f:?}") };
        assert_eq!(*step, InstallStep::Dependencies);
        assert!(
            detail.contains("exit code 1") && detail.contains("target not found: layer-shell-qt"),
            "{detail}"
        );
        assert!(!detail.contains(FAIL_MARK) && !detail.contains(STEP_MARK));
        // ⭐ 16.09.2026: a refusal at this step NAMES to the person what was missing
        // and says the system is untouched — the generic phrase "failed: installing
        // missing components" is no longer here.
        assert!(r.message.contains(&inst::dependencies_failed_problem(RU)), "{}", r.message);
        // The words are English (the catalog's source language); the Russian is
        // checked word for word by `tests/installer_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let message = inst::dependencies_failed_problem(RU);
        for word in ["Qt6", "wallpaper layer", "video player", "Vulkan loader"] {
            assert!(message.contains(word), "не названо «{word}»: {message}");
        }
        assert!(message.contains("Nothing in the system was changed"), "{message}");
        // The raw package name — only in the developer details.
        assert!(!r.message.contains("layer-shell-qt"), "{}", r.message);
        assert!(!sink.has(&format!("✅ {}", InstallStep::Dependencies.progress_line(RU).trim_end_matches('…'))));
        cleaned_last(&io);
    }

    #[test]
    fn code_127_from_inside_started_script_is_a_step_failure() {
        // "command not found" inside the script also gives 127 — but the script ran.
        let io = FakeIo::new(|| out(127, &marks(&["deps", "files"]), &format!("cp: not found\n{FAIL_MARK}files\n")));
        let (r, sink) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(sink.has(&format!("✅ {}", InstallStep::Dependencies.progress_line(RU).trim_end_matches('…'))));
        cleaned_last(&io);
    }

    #[test]
    fn timeout_and_missing_password_prompt_are_named_and_cleaned() {
        let io = FakeIo::new(|| RunOut {
            outcome: RunOutcome::Timeout,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        });
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::TimedOut { secs: 600 });
        cleaned_last(&io);

        let io = FakeIo::new(RunOut::missing);
        let (r, _) = install(&io, &request());
        assert_eq!(failure(&r), InstallFailure::PasswordPromptMissing);
        cleaned_last(&io);
    }

    #[test]
    fn script_ok_but_nothing_in_place_is_not_success() {
        let mut io = FakeIo::ok();
        io.lands = false;
        let (r, _) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::SystemInstall, .. }));
        cleaned_last(&io);
    }

    #[test]
    fn classify_by_fact_not_by_code_alone() {
        let c = |o: RunOut| classify_script_run(&o, 600);
        assert_eq!(c(out(0, &all_marks(), "")), Ok(()));
        assert_eq!(c(out(126, "", "")), Err(InstallFailure::PasswordDenied));
        assert_eq!(c(out(127, "", "")), Err(InstallFailure::PasswordDenied));
        assert_eq!(c(out(1, "", "Error executing command as another user: Not authorized")), Err(InstallFailure::PasswordDenied));
        assert!(matches!(
            c(out(3, "", "boom")),
            Err(InstallFailure::StepFailed { step: InstallStep::SystemInstall, .. })
        ));
        assert!(matches!(
            c(out(126, &marks(&["deps", "files"]), "")),
            Err(InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. })
        ));
    }

    // ── Cleanup on a preparation error and on cancel (fix 2) ────────

    #[test]
    fn prepare_failure_cleans_up_and_never_asks_password() {
        let mut io = FakeIo::ok();
        io.fail_copy_tree = true;
        let (r, _) = install(&io, &request());
        assert_eq!(
            failure(&r),
            InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: "диск переполнен".into() }
        );
        assert!(io.runs().is_empty(), "пароль не спрашивали");
        cleaned_last(&io);
    }

    #[test]
    fn temp_dir_failure_touches_nothing_else() {
        let mut io = FakeIo::ok();
        io.fail_temp_dir = true;
        let (r, _) = install(&io, &request());
        assert!(matches!(failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert_eq!(io.calls(), vec![Call::TempDir]);
    }

    #[test]
    fn cancel_before_password_cleans_up_and_system_untouched() {
        let io = FakeIo::ok();
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = install_to_system(&io, &request(), Arc::new(RecSink::default()), &cancel);
        assert!(r.cancelled() && !r.ok());
        assert!(io.runs().is_empty());
        cleaned_last(&io);
    }

    #[test]
    fn immutable_system_refuses_before_any_side_effect() {
        let io = FakeIo::ok();
        let mut req = request();
        req.distro_id = "steamos".into();
        let (r, _) = install(&io, &req);
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    // ── File permissions (fix 4) ────────────────────────────────────

    #[test]
    fn modes_dirs_755_files_644_executables_755() {
        assert_eq!(payload_mode(true, false), 0o755);
        assert_eq!(payload_mode(false, false), 0o644);
        assert_eq!(payload_mode(false, true), 0o755);

        let io = FakeIo::ok();
        install(&io, &request());
        let calls = io.calls();
        let pkg = format!("{STAGE}/lib/rexon");
        let item = |rel: &str| {
            calls.iter().find_map(|c| match c {
                Call::TreeItem(p, m) if *p == format!("{pkg}/{rel}") => Some(*m),
                _ => None,
            })
        };
        assert_eq!(item("themes.json"), Some(0o644));
        assert_eq!(item("app.py"), None, "исходники Python не ставятся");
        assert_eq!(item(".hidden.json"), Some(0o644), "скрытые файлы не теряются");
        assert_eq!(item("tools"), Some(0o755));
        assert_eq!(item("tools/run.sh"), Some(0o755));
        assert_eq!(item("__pycache__"), None, "кеши байткода не ставятся");
        assert_eq!(item("__pycache__/app.cpython.pyc"), None);

        let copied = |dst: &str| {
            calls.iter().find_map(|c| match c {
                Call::CopyFile(_, d, m) if d == dst => Some(*m),
                _ => None,
            })
        };
        let st = StagePaths::new(STAGE);
        assert_eq!(copied(&st.layout().main_window()), Some(0o755));
        assert_eq!(copied(&st.layout().theme_shot()), Some(0o755), "помощник снимков — рядом с окном");
        assert_eq!(copied(&st.layout().filter_lib()), Some(0o755));
        assert_eq!(copied(&st.layout().wallpaper_host()), Some(0o755));
        assert_eq!(copied(&format!("{}/rexon_icon_16.png", st.icons_dir())), Some(0o644));
        assert_eq!(io.written(&st.desktop()).unwrap().1, 0o644);
        assert_eq!(io.written(&st.layout().launcher()).unwrap().1, 0o755);
        let h = st.helpers();
        assert_eq!(io.written(&h.tmp_pkg_helper).unwrap().1, 0o755);
        assert_eq!(io.written(&h.tmp_pkg_policy).unwrap().1, 0o644);
        for c in &calls {
            if let Call::MakeDirs(p, m) = c {
                if p.starts_with(STAGE) {
                    assert_eq!(*m, Some(0o755), "каталог поставки {p}");
                }
            }
        }

        let script = script_of(&io);
        assert!(!script.contains("chmod -R 755"));
        assert!(script.contains(&format!("cp -r --preserve=mode {STAGE}/lib/. /usr/local/lib/rexon/\n")));
        assert!(script.contains("chown -R root:root /usr/local/lib/rexon\n"));
    }

    // ── Rust window and fallback to Python (fix 5) ──────────────────

    #[test]
    fn installs_rust_window_and_menu_entry_points_to_it() {
        let io = FakeIo::ok();
        let (r, _) = install(&io, &request());
        let target = InstallLayout::system();
        assert_eq!(r.entry, Some(LaunchEntry::MainWindow(target.main_window())));
        let desktop = io.written(&StagePaths::new(STAGE).desktop()).unwrap().0;
        assert!(desktop.contains(&format!("Exec={}\n", target.main_window())), "{desktop}");
        // 16.09.2026: there is no "classic window" item in the desktop entry any
        // more — nothing to install it with, the `rexon` command leads to the same Rust window.
        assert!(!desktop.contains("Classic"), "{desktop}");

        let script = script_of(&io);
        for dst in [target.launcher(), target.wallpaper_host(), target.splash_overlay()] {
            assert!(script.contains(&format!(" {dst}\n")), "{dst} ставится");
        }
    }

    /// ⭐ 16.09.2026: a payload without the window binary is refused BEFORE the
    /// first side effect. Previously such an install went through "successfully"
    /// and pointed the desktop entry at the classic window; there is no Python in
    /// the payload any more, and the `rexon` command itself leads to the window
    /// binary — the install would put into the system a launcher pointing into the void.
    #[test]
    fn without_the_window_binary_the_install_refuses_and_touches_nothing() {
        for mode in [InstallMode::System, InstallMode::User] {
            let io = FakeIo::ok();
            let mut req = request();
            req.payload.main_window = None;
            let sink = Arc::new(RecSink::default());
            let r = super::install(&io, mode, &req, sink, &CancelToken::new());
            assert!(!r.ok() && !r.cancelled(), "{mode:?}: {r:?}");
            assert!(
                matches!(r.failure(), Some(InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. })),
                "{mode:?}: {r:?}"
            );
            assert_eq!(r.entry, None);
            assert!(io.calls().is_empty(), "{mode:?}: система не тронута");
        }

        // Portable mode doesn't need the binary — it doesn't try to install anything.
        let io = FakeIo::ok();
        let mut req = request();
        req.payload.main_window = None;
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::Portable, &req, sink, &CancelToken::new());
        assert!(r.ok(), "{r:?}");
    }

    /// The manual install's `rexon` launcher leads to the window binary — as with
    /// the distribution package, but WITHOUT the package marker: otherwise mode
    /// selection would consider the install portable and the wizard would never open again.
    #[test]
    fn the_command_leads_to_the_window_binary_without_the_package_mark() {
        let io = FakeIo::ok();
        let (_, _) = install(&io, &request());
        let target = InstallLayout::system();
        let text = io.written(&StagePaths::new(STAGE).layout().launcher()).unwrap().0;
        assert_eq!(text, inst::system_launcher(&target.lib_dir));
        assert!(text.starts_with("#!/bin/sh\n"), "{text}");
        assert!(text.contains(&format!("exec {} \"$@\"\n", target.main_window())), "{text}");
        assert!(!text.contains(inst::PACKAGED_ENV), "{text}");
        assert!(!text.contains("python"), "{text}");

        // The fallback variable redirects the desktop entry, but not the launcher:
        // it has nowhere else to lead, there is no classic window in the payload.
        let io = FakeIo::ok();
        let mut req = request();
        req.entry_env = Some("python".into());
        let (r, _) = install(&io, &req);
        assert_eq!(r.entry, Some(LaunchEntry::Python));
        assert_eq!(
            io.written(&StagePaths::new(STAGE).layout().launcher()).unwrap().0,
            inst::system_launcher(&target.lib_dir)
        );
    }

    // ── The layout is one (fix 6) ───────────────────────────────────

    #[test]
    fn layout_is_one_for_package_and_manual_install() {
        let pkg = InstallLayout::package();
        assert_eq!(pkg.main_window(), inst::PACKAGE_MAIN_WINDOW);
        assert_eq!(pkg.qml_modules(), qm::SYSTEM_MODULE_DIRS[0]);
        assert!(inst::package_data_dir_candidates("", "/h").contains(&pkg.package_dir()));

        let sys = InstallLayout::system();
        assert_eq!(Some(sys.clone()), InstallLayout::for_mode(InstallMode::System, "/h", None));
        assert!(
            inst::package_data_dir_candidates("", "/h").contains(&sys.package_dir()),
            "Rust-окно находит данные установки в систему"
        );
        assert!(
            inst::main_window_candidates(None, "", &[]).contains(&sys.main_window()),
            "выбор входа находит бинарь окна установки в систему"
        );
        assert!(
            qm::module_dirs(None, "/h", "", &[]).contains(&sys.qml_modules()),
            "QML-модули установки в систему ищутся"
        );
        assert!(
            inst::system_launcher(&sys.lib_dir).contains(&format!("exec {}", sys.main_window())),
            "команда `rexon` ведёт на бинарь окна этой же раскладки"
        );

        // The home folder has the same shape, and the lookup knows it (fix 11).
        let user = InstallLayout::for_mode(InstallMode::User, "/h", None).unwrap();
        assert_eq!(user.main_window(), "/h/.local/lib/rexon/rexon-main-window");
        assert_eq!(user.wallpaper_host(), "/h/.local/bin/rexon-wallpaper-host");
        assert_eq!(
            InstallLayout::for_mode(InstallMode::User, "/h", Some("/xdg")),
            Some(user.clone()),
            "XDG_DATA_HOME двигает ярлыки и значки, но не программу"
        );
        assert!(
            inst::main_window_candidates(None, "/h", &[]).contains(&user.main_window()),
            "выбор входа находит бинарь окна установки в домашнюю папку"
        );
        assert!(
            qm::module_dirs(None, "/h", "", &[]).contains(&user.qml_modules()),
            "QML-модули установки в домашнюю папку ищутся"
        );
        assert!(
            inst::package_data_dir_candidates("", "/h").contains(&user.package_dir()),
            "Rust-окно находит данные установки в домашнюю папку"
        );
        assert!(
            !inst::main_window_candidates(None, "", &[]).iter().any(|c| c.contains(".local")),
            "без домашней папки кандидата нет — относительный путь не ищется"
        );
        assert!(InstallLayout::for_mode(InstallMode::Portable, "/h", None).is_none());

        // The package puts our .so in the same place (scripts/package-build), and
        // doesn't ship the former rexon-gui library at all.
        let pkgbuild = include_str!("../../../scripts/package-build");
        assert!(pkgbuild.contains(&format!("\"$qmod/{}/{}\"", qm::RUST_LIB_SUBDIR, qm::FILTER_LIB_NAME)));
        assert!(!pkgbuild.contains(RETIRED_GUI_LIB));

        // Extensions are taken from the package inside <lib>, not from <lib> directly.
        let io = FakeIo::ok();
        install(&io, &request());
        assert!(script_of(&io).contains(&format!("EXT_SRC=/usr/local/lib/rexon/rexon/\"$DE_DIR\"/{}", inst::EXT_ID)));
    }

    #[test]
    fn payload_from_bundle_follows_the_same_layout() {
        let io = FakeIo::ok();
        assert_eq!(PayloadSource::from_bundle(&io, "/src"), payload());
        assert_eq!(PayloadSource::for_running(&io, "/src/rexon-main-window", "", "/h", None), payload());
    }

    // ── The no-password rule — only with consent (fix 7) ────────────

    #[test]
    fn no_consent_no_passwordless_rule() {
        let io = FakeIo::ok();
        let req = request();
        assert!(!req.passwordless_rule, "по умолчанию выключено");
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        assert!(io.written(&st.helpers().tmp_de_rules).is_none(), "файла правила нет");
        let script = script_of(&io);
        assert!(!script.contains(&format!("{} {}", st.helpers().tmp_de_rules, plan::REXON_POLKIT_RULES_PATH)));
        assert!(!script.contains("rexon_step rule"));
        assert!(script.contains(&format!("rm -f {}", plan::REXON_POLKIT_RULES_PATH)), "молчаливое прежнее снимается");
        // All helpers are installed in this case: eight old files and two of the
        // privilege session (binary and policy).
        assert_eq!(script.matches(&format!("{}/", st.polkit_dir())).count(), 10);
    }

    /// ⛔ "One password per run" (B11): the manual install puts the privilege
    /// session helper as the payload binary (755) and the `org.rexon.session`
    /// policy (644) in the helpers step; the policy points to the manual install path.
    #[test]
    fn installs_session_helper_and_its_policy() {
        let io = FakeIo::ok();
        let (r, _) = install(&io, &request());
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        let copied = io.calls().into_iter().find_map(|c| match c {
            Call::CopyFile(src, dst, m) if dst == st.priv_helper() => Some((src, m)),
            _ => None,
        });
        assert_eq!(copied, Some(("/src/bin/rexon-priv-helper".to_string(), 0o755)));
        let (policy, mode) = io.written(&st.session_policy()).expect("политика записана");
        assert_eq!(mode, 0o644);
        assert_eq!(policy, plan::session_policy_xml());
        assert!(policy.contains(plan::SESSION_HELPER_PATH));
        let script = script_of(&io);
        let at = script.find("rexon_step helpers").expect("шаг помощников");
        let tail = &script[at..script.find("rexon_step caches").unwrap()];
        assert!(tail.contains(&format!("install -Dm755 {} {}\n", st.priv_helper(), plan::SESSION_HELPER_PATH)), "{tail}");
        assert!(tail.contains(&format!("install -Dm644 {} {}\n", st.session_policy(), plan::SESSION_POLICY_PATH)), "{tail}");
        assert!(!script.contains(&format!("rm -f {}", plan::SESSION_HELPER_PATH)));
    }

    /// A payload without the session helper: the policy is not written, and the
    /// old helper is removed — a window of the new build would reject it anyway
    /// («Rexon was updated»), and without the file operations take the old path.
    #[test]
    fn payload_without_session_helper_removes_the_old_one() {
        let io = FakeIo::ok();
        let mut req = request();
        req.payload.priv_helper = None;
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        assert!(io.written(&st.session_policy()).is_none());
        let script = script_of(&io);
        assert!(script.contains(&format!("rm -f {}\n", plan::SESSION_HELPER_PATH)), "{script}");
        assert!(!script.contains(plan::SESSION_POLICY_PATH), "{script}");
    }

    /// The payload finds the helper where the build puts it: in the developer
    /// tree — next to the window binary, in the archive — in `bin/`.
    #[test]
    fn payload_finds_session_helper() {
        let mut io = FakeIo::ok();
        let release = inst::DEV_TREE_MAIN_WINDOW_REL.rsplit_once('/').unwrap().0;
        let dev = format!("/repo/{release}/{}", plan::SESSION_HELPER_NAME);
        io.files.insert(dev.clone());
        io.files.insert("/b/bin/rexon-priv-helper".into());
        assert_eq!(PayloadSource::from_dev_tree(&io, "/repo", "/home/u", None).priv_helper, Some(dev));
        assert_eq!(PayloadSource::from_bundle(&io, "/b").priv_helper, Some("/b/bin/rexon-priv-helper".into()));
        assert_eq!(PayloadSource::from_bundle(&io, "/nothing").priv_helper, None);
    }

    #[test]
    fn with_consent_rule_is_a_separate_narrow_step() {
        let io = FakeIo::ok();
        let mut req = request();
        req.passwordless_rule = true;
        let (r, _) = install(&io, &req);
        assert!(r.ok());
        let st = StagePaths::new(STAGE);
        let (rules, mode) = io.written(&st.helpers().tmp_de_rules).expect("правило записано");
        assert_eq!(mode, 0o644);
        assert_eq!(rules, plan::passwordless_rules_js(PkgFamily::Arch));
        let script = script_of(&io);
        let at = script.find("rexon_step rule").expect("отдельный шаг");
        assert!(script[at..].contains(&format!("{} {}", st.helpers().tmp_de_rules, plan::REXON_POLKIT_RULES_PATH)));
        assert!(!script.contains(&format!("rm -f {}", plan::REXON_POLKIT_RULES_PATH)));
    }

    #[test]
    fn passwordless_rule_is_narrow_and_knows_admin_group_of_family() {
        let arch = plan::passwordless_rules_js(PkgFamily::Arch);
        for action in plan::PASSWORDLESS_ACTIONS {
            assert!(arch.contains(&format!("\"{action}\"")));
        }
        assert!(!arch.contains(plan::PKG_ACTION), "пакеты по имени — только с паролем");
        assert!(!arch.contains("indexOf(\"org.rexon.\") === 0"), "не всё семейство org.rexon.*");
        assert!(arch.contains("subject.isInGroup(\"wheel\")") && !arch.contains("\"sudo\""));
        let debian = plan::passwordless_rules_js(PkgFamily::Debian);
        assert!(debian.contains("subject.isInGroup(\"sudo\") || subject.isInGroup(\"admin\")"));
        assert!(!debian.contains("\"wheel\""), "в Debian группы wheel у администраторов нет");
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Solus), &["sudo"]);
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Fedora), &["wheel"]);
        assert_eq!(plan::polkit_admin_groups(PkgFamily::Unknown), &["wheel"]);
    }

    #[test]
    fn classic_installer_script_sets_rule_only_with_consent() {
        let h = StagePaths::new(STAGE).helpers();
        let mut p = inst::InstallScriptParams {
            lib_dir: inst::LIB_DIR.into(),
            install_dir: inst::INSTALL_DIR.into(),
            desktop_dir: inst::DESKTOP_DIR.into(),
            tmp_pkg: "/tmp/p".into(),
            tmp_launcher: "/tmp/l".into(),
            tmp_desktop: "/tmp/d".into(),
            tmp_recover: "/tmp/r".into(),
            icons: IconSet::default(),
            helpers: h.clone(),
            dep_cmd: String::new(),
            passwordless_rule: false,
        };
        let install_line = format!("install -Dm644 {} {}", h.tmp_de_rules, plan::REXON_POLKIT_RULES_PATH);
        assert!(!inst::system_install_script(&p).contains(&install_line));
        p.passwordless_rule = true;
        assert!(inst::system_install_script(&p).contains(&install_line));
        assert_eq!(inst::system_install_cmd("/tmp/x.sh").timeout_secs, Some(600));
    }

    // ── Miscellaneous ───────────────────────────────────────────────

    /// Dependencies are asked of the dynamic linker for the payload's own files,
    /// and the interpreter is never started.
    #[test]
    fn runtime_deps_are_asked_of_the_dynamic_linker() {
        let ran = |io: &FakeIo| -> Vec<Vec<String>> {
            io.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Run(a, t) => {
                        assert_eq!(t, Some(LDD_TIMEOUT_SECS), "потолок пробы: {a:?}");
                        Some(a)
                    }
                    _ => None,
                })
                .collect()
        };

        // Nothing to check with: the system has no `ldd` — stay quiet and call nobody.
        let io = FakeIo::ok();
        assert!(runtime_deps_present(&io, &payload()));
        assert!(io.calls().is_empty());

        // All libraries present: exactly the payload files were asked about.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        io.reply = Box::new(|_| out(0, "\tlibQt6Core.so.6 => /usr/lib/libQt6Core.so.6\n", ""));
        assert!(runtime_deps_present(&io, &payload()));
        assert_eq!(
            ran(&io),
            [
                "/src/rexon-main-window",
                "/src/bin/rexon-wallpaper-host",
                "/src/bin/rexon-splash-overlay",
                "/src/qml_modules/waywallen/control/librexon_qt_filter.so",
            ]
            .map(|p| vec![LDD.to_string(), p.to_string()])
        );

        // One unresolved library — and dependencies get installed.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        io.reply = Box::new(|argv: &[String]| {
            if argv[1].ends_with("rexon-splash-overlay") {
                out(0, "\tlibmpv.so.2 => not found\n", "")
            } else {
                out(0, "\tlibQt6Core.so.6 => /usr/lib/libQt6Core.so.6\n", "")
            }
        });
        assert!(!runtime_deps_present(&io, &payload()));
        let st = StagePaths::new(STAGE);
        assert!(!system_script_params(&io, &request(), &st).dep_lines.is_empty());

        // The payload has no binaries — nothing to check.
        let mut io = FakeIo::ok();
        io.tools.insert(LDD.into());
        let empty = PayloadSource {
            main_window: None,
            wallpaper_host: None,
            splash_overlay: None,
            filter_lib: None,
            ..payload()
        };
        assert!(runtime_deps_present(&io, &empty));
        assert!(io.calls().is_empty());
    }

    #[test]
    fn deps_lines_port_install_deps_cmd() {
        assert_eq!(deps_script_lines(PkgFamily::Arch, true, true), "");
        let all = deps_script_lines(PkgFamily::Arch, false, false);
        assert_eq!(all.lines().next(), Some(inst::dep_install_cmd(PkgFamily::Arch).as_str()));
        assert!(all.lines().nth(1).unwrap().ends_with(" || true"));
        assert_eq!(deps_script_lines(PkgFamily::Nix, false, false), "", "NixOS: ставить нечем");
    }

    /// ⭐⭐ Guard of the 16.09.2026 split, and it is checked for ALL families, not
    /// just Arch: required dependencies go on THEIR OWN line without cover (a
    /// failure aborts the install — as intended), recommended ones — on their own,
    /// with `|| true`. The recommended names in eleven families are written out by
    /// naming rules and not verified live: a typo there must not cost the person
    /// the whole install.
    #[test]
    fn every_family_sends_must_have_and_nice_to_have_in_separate_lines() {
        for family in [
            PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
            PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
            PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
        ] {
            let name = family.as_str();
            // The style switcher is already installed — only the two dependency tiers
            // remain in the text.
            let text = deps_script_lines(family, false, true);
            let must = inst::dep_install_cmd(family);
            let want = inst::optional_dep_install_cmd(family);
            if must.is_empty() {
                // NixOS and "manager not recognised" — nothing to install either tier with;
                // inventing a command is not allowed.
                assert!(want.is_empty(), "{name}: желательные есть, а обязательных нет");
                assert_eq!(text, "", "{name}: ставить нечем — строк нет");
                continue;
            }
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines.len(), 2, "{name}: разрядов не два: {text}");
            assert_eq!(lines[0], must, "{name}: первая строка — обязательные");
            assert!(
                !lines[0].contains("|| true"),
                "{name}: обязательные прикрыты — отказ стал бы молчаливым"
            );
            assert_eq!(lines[1], format!("{want} || true"), "{name}: вторая строка — желательные");
            // The lines don't overlap: no name is installed twice.
            for pkg in inst::optional_dep_packages(family) {
                assert!(
                    !lines[0].split_whitespace().any(|w| w == pkg),
                    "{name}: «{pkg}» попал и в обязательные"
                );
            }
        }
    }

    /// Recommended ones go exactly when the required ones do: the `ldd` probe
    /// answer is one for the whole payload, and "the linker is happy" means there
    /// is no reason to call the package manager for either tier.
    #[test]
    fn nice_to_have_packages_ride_with_the_must_have_ones() {
        assert_eq!(deps_script_lines(PkgFamily::Arch, true, true), "");
        // Only the style switcher is needed — and it alone has `|| true`.
        let only_style = deps_script_lines(PkgFamily::Arch, true, false);
        assert_eq!(only_style, format!("{} || true", inst::qt_style_install_cmd(PkgFamily::Arch)));
        // Everything at once — three lines, the last two covered.
        let all = deps_script_lines(PkgFamily::Arch, false, false);
        let lines: Vec<&str> = all.lines().collect();
        assert_eq!(lines.len(), 3, "{all}");
        assert!(!lines[0].contains("|| true"));
        assert!(lines[1].ends_with(" || true") && lines[2].ends_with(" || true"));
    }

    #[test]
    fn plan_has_only_known_steps_and_cleans_stage() {
        let st = StagePaths::new(STAGE);
        let io = FakeIo::ok();
        let steps = install_plan(&system_script_params(&io, &request(), &st));
        assert!(check_plan(&steps));
        assert_eq!(steps.last(), Some(&PlanStep::RemoveFiles { paths: vec![STAGE.to_string()] }));
        assert!(!check_plan(&[PlanStep::EnsureGentooOverlay { overlay: "x".into() }]));
        assert!(!stage_path_is_plain("/tmp/with space/x"));
        // Every script marker is a known step, and all markers from the table are in the script.
        let script = install_script(&system_script_params(&io, &request(), &st));
        for (id, _) in SCRIPT_MARKS.iter().filter(|(id, _)| *id != "rule") {
            assert!(script.contains(&format!("\nrexon_step {id}\n")), "{id}");
        }
        // Caches in the script come from the same table as for the home folder install.
        for group in cache_refresh_groups(inst::DESKTOP_DIR, "/usr/share/icons/hicolor") {
            for argv in &group {
                assert!(script.contains(&format!("{} 2>/dev/null", argv.join(" "))), "{argv:?}");
            }
        }
        assert!(script.contains(&format!("'{GNOME_EXTENSION_SRC}|/usr/share/gnome-shell/extensions'")));
    }

    #[test]
    fn progress_lines_come_from_wizard_texts_without_jargon() {
        let io = FakeIo::new(|| out(1, &marks(&["deps"]), &format!("{FAIL_MARK}deps")));
        let (_, sink) = install(&io, &request());
        let io2 = FakeIo::ok();
        let (_, sink2) = install(&io2, &request());
        for (stage, m) in sink.lines.lock().unwrap().iter().chain(sink2.lines.lock().unwrap().iter()) {
            for bad in ["pkexec", "polkit", "sudo", ".desktop", "PySide", "/usr", "rules.d"] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
    }

    // ── Home folder: the real FS ONLY inside the test's temp dir ──

    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SCRATCH_N: AtomicUsize = AtomicUsize::new(0);

    /// A substitute executor with a real FS inside its own temporary directory.
    /// EVERY write, rename and delete path is checked against the root — leaving
    /// it fails the test, the live HOME is unreachable. External commands are NOT
    /// executed, only recorded.
    struct ScratchIo {
        base: String,
        env: HashMap<String, String>,
        tools: HashSet<String>,
        runs: RefCell<Vec<Vec<String>>>,
        /// A tree copy failure whose destination path contains the string.
        fail_copy_tree_into: Option<String>,
        /// One rename failure INTO this path.
        fail_rename_once_into: RefCell<Option<String>>,
    }

    impl ScratchIo {
        fn new() -> Self {
            let n = SCRATCH_N.fetch_add(1, Ordering::SeqCst);
            let base = std::env::temp_dir().join(format!("rexon-g3b-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).expect("временный каталог теста");
            let base = base.canonicalize().unwrap().to_string_lossy().into_owned();
            let home = format!("{base}/home");
            std::fs::create_dir_all(&home).unwrap();
            let env = [
                ("HOME".to_string(), home.clone()),
                ("PATH".to_string(), format!("/usr/bin:{home}/.local/bin")),
            ]
            .into_iter()
            .collect();
            let tools = ["update-desktop-database", "gtk-update-icon-cache", "kbuildsycoca6", GLIB_COMPILE_SCHEMAS]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let io = ScratchIo {
                base,
                env,
                tools,
                runs: RefCell::new(Vec::new()),
                fail_copy_tree_into: None,
                fail_rename_once_into: RefCell::new(None),
            };
            for (rel, text, mode) in [
                ("src/rexon/app.py", "print()", 0o644),
                ("src/rexon/themes.json", "{}", 0o644),
                ("src/rexon/qt_wallpaper_engine/qml_ui/PageHost.qml", "import QtQuick", 0o644),
                ("src/rexon/.hidden.json", "{}", 0o644),
                ("src/rexon/__pycache__/app.cpython.pyc", "x", 0o644),
                ("src/rexon/tools/run.sh", "#!/bin/sh", 0o755),
                ("src/rexon/gnome-extension/rexon-wallpaper@rexon.app/extension.js", "//", 0o644),
                ("src/rexon/gnome-extension/rexon-wallpaper@rexon.app/schemas/org.rexon.gschema.xml", "<x/>", 0o644),
                ("src/rexon/cinnamon-extension/rexon-wallpaper@rexon.app/extension.js", "//", 0o644),
                ("src/rexon-main-window", "ELF", 0o755),
                ("src/bin/rexon-wallpaper-host", "ELF", 0o755),
                ("src/bin/rexon-splash-overlay", "ELF", 0o755),
                ("src/qml_modules/Qcm/Material/libqml_materialplugin.so", "so", 0o755),
                // A stale copy of the former library — the install doesn't take it.
                ("src/qml_modules/waywallen/control/librexon_gui.so", "so", 0o755),
                ("src/qml_modules/waywallen/control/librexon_qt_filter.so", "so", 0o755),
                ("src/rexon_icon_16.png", "png", 0o644),
                ("src/rexon_icon_128.png", "png", 0o644),
                ("src/rexon_icon.svg", "<svg/>", 0o644),
            ] {
                io.put(rel, text, mode);
            }
            io
        }

        fn home(&self) -> String {
            self.env["HOME"].clone()
        }

        fn xdg(&self) -> Option<&str> {
            self.env.get("XDG_DATA_HOME").map(String::as_str).filter(|x| !x.is_empty())
        }

        fn layout(&self) -> InstallLayout {
            InstallLayout::for_mode(InstallMode::User, &self.home(), self.xdg()).unwrap()
        }

        fn paths(&self) -> inst::InstallPaths {
            inst::install_paths(InstallMode::User, &self.home(), self.xdg())
        }

        fn request(&self) -> UserInstallRequest {
            UserInstallRequest::new(PayloadSource::from_bundle(self, &format!("{}/src", self.base)), RU)
        }

        fn guard(&self, path: &str) {
            assert!(
                path.starts_with(&format!("{}/", self.base)) && !path.split('/').any(|s| s == ".."),
                "запись вне временного каталога теста: {path}"
            );
        }

        /// A file inside the test's temporary directory (path from the root).
        fn put(&self, rel: &str, text: &str, mode: u32) {
            let path = format!("{}/{rel}", self.base);
            std::fs::create_dir_all(parent_dir(&path)).unwrap();
            std::fs::write(&path, text).unwrap();
            set_mode(Path::new(&path), mode).unwrap();
        }

        fn runs(&self) -> Vec<Vec<String>> {
            self.runs.borrow().clone()
        }
    }

    impl Drop for ScratchIo {
        fn drop(&mut self) {
            if self.base.contains("rexon-g3b-test-") {
                let _ = std::fs::remove_dir_all(&self.base);
            }
        }
    }

    impl SystemIo for ScratchIo {
        fn exists(&self, path: &str) -> bool {
            Path::new(path).exists()
        }
        fn is_file(&self, path: &str) -> bool {
            Path::new(path).is_file()
        }
        fn is_dir(&self, path: &str) -> bool {
            Path::new(path).is_dir()
        }
        fn is_executable(&self, path: &str) -> bool {
            std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        fn realpath(&self, path: &str) -> Option<String> {
            std::fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
        }
        fn read_text(&self, path: &str) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }
        fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
            let entries = std::fs::read_dir(dir).ok()?;
            Some(entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        }
        fn which(&self, name: &str) -> bool {
            self.tools.contains(name)
        }
        fn run(&self, argv: &[String], _timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
            self.runs.borrow_mut().push(argv.to_vec());
            out(0, "", "")
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, _content: &str) -> Result<(), String> {
            panic!("write_text исполнитель установки не зовёт: {path}")
        }
        fn copy_file(&self, src: &str, _dst: &str) -> Result<(), String> {
            panic!("copy_file исполнитель установки не зовёт: {src}")
        }
    }

    impl InstallIo for ScratchIo {
        fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
            let parent = parent.map(str::to_string).unwrap_or_else(|| format!("{}/tmp", self.base));
            self.guard(&format!("{parent}/{prefix}"));
            std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
            for k in 0.. {
                let path = format!("{parent}/{prefix}{k}");
                if std::fs::create_dir(&path).is_ok() {
                    set_mode(Path::new(&path), 0o700)?;
                    return Ok(path);
                }
            }
            unreachable!()
        }
        fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
            self.guard(src);
            self.guard(dst);
            let mut fail = self.fail_rename_once_into.borrow_mut();
            if fail.as_deref() == Some(dst) {
                *fail = None;
                return Err("переименование не удалось".into());
            }
            std::fs::rename(src, dst).map_err(|e| e.to_string())
        }
        fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
            self.guard(path);
            std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
            match mode {
                Some(m) => set_mode(Path::new(path), m),
                None => Ok(()),
            }
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.guard(path);
            std::fs::write(path, content).map_err(|e| e.to_string())?;
            set_mode(Path::new(path), mode)
        }
        fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
            self.guard(dst);
            std::fs::copy(src, dst).map_err(|e| e.to_string())?;
            set_mode(Path::new(dst), mode)
        }
        fn copy_tree(
            &self,
            src: &str,
            dst: &str,
            skip: &dyn Fn(&str) -> bool,
            mode: &dyn Fn(bool, bool) -> u32,
        ) -> Result<usize, String> {
            self.guard(dst);
            if self.fail_copy_tree_into.as_deref().is_some_and(|f| dst.contains(f)) {
                return Err("диск переполнен".into());
            }
            let mut count = 0;
            walk_copy(Path::new(src), Path::new(dst), skip, mode, &mut count)?;
            Ok(count)
        }
        fn remove_file(&self, path: &str) {
            self.guard(path);
            let _ = std::fs::remove_file(path);
        }
        fn remove_tree(&self, path: &str) {
            self.guard(path);
            let _ = std::fs::remove_dir_all(path);
        }
    }

    fn user_install(io: &ScratchIo) -> (InstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = install_for_user(io, &io.request(), sink.clone(), &CancelToken::new());
        (r, sink)
    }

    /// Files and links under a directory (without the directories themselves), in order.
    fn tree(dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    found.push(e.path().to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        found
    }

    fn mode_of(path: &str) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn read(path: &str) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// The install temporary directory is removed on any outcome.
    fn no_stage_left(io: &ScratchIo) {
        let parent = parent_dir(&io.layout().lib_dir).to_string();
        let left: Vec<String> = io
            .list_dir(&parent)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.starts_with(USER_STAGE_PREFIX))
            .collect();
        assert!(left.is_empty(), "временный каталог остался: {left:?}");
        assert!(tree(&io.home()).iter().all(|f| !f.ends_with(".rexon-new")), "временные имена файлов убраны");
    }

    fn user_failure(r: &InstallResult) -> InstallFailure {
        r.failure().cloned().unwrap_or_else(|| panic!("ждали неудачу: {r:?}"))
    }

    #[test]
    fn user_install_lays_out_one_tree_and_uninstall_finds_it() {
        let mut io = ScratchIo::new();
        let xdg = format!("{}/xdg-data", io.home());
        io.env.insert("XDG_DATA_HOME".into(), xdg.clone());
        let (r, sink) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed { mode: InstallMode::User, fell_back: None, command_dir_not_in_path: None })
        );
        assert_eq!(r.message, inst::install_done_line(InstallMode::User, RU));
        let l = io.layout();
        let p = io.paths();
        assert_eq!(r.entry, Some(LaunchEntry::MainWindow(l.main_window())));

        // The program — per the package layout, permissions laid out.
        let pkg = l.package_dir();
        assert_eq!(mode_of(&format!("{pkg}/themes.json")), 0o644);
        assert_eq!(mode_of(&format!("{pkg}/tools/run.sh")), 0o755);
        assert!(io.is_file(&format!("{pkg}/.hidden.json")), "скрытые файлы не теряются");
        assert!(!io.exists(&format!("{pkg}/__pycache__")), "кеши байткода не ставятся");
        // There are no Python sources in the install, yet the data directory is
        // still recognised — by the vendored QML of the wallpaper tab.
        assert!(!io.exists(&format!("{pkg}/app.py")), "исходники Python не ставятся");
        assert!(io.is_file(&format!("{pkg}/{}", inst::PACKAGE_MARKER_REL)));
        assert!(
            inst::package_data_dir_candidates("", &io.home()).contains(&pkg),
            "окно ищет данные там, куда установка их положила"
        );
        for exe in [l.main_window(), l.wallpaper_host(), l.splash_overlay(), l.filter_lib()] {
            assert!(io.is_executable(&exe), "{exe}");
        }
        // A stale former rexon-gui library in the payload's module directory doesn't
        // make it into the install.
        assert!(!io.exists(&format!("{}/{RETIRED_GUI_LIB}", l.rust_lib_dir())));
        assert!(io.is_file(&format!("{}/{}", l.qml_modules(), qm::QCM_MATERIAL_REL)));

        // The launcher and the desktop entry lead to the window binary of this same install.
        assert_eq!(read(&l.launcher()), inst::user_launcher(&l.lib_dir));
        assert!(read(&l.launcher()).contains(&format!("exec {} \"$@\"", l.main_window())));
        assert_eq!(mode_of(&l.launcher()), 0o755);
        let desktop_dir = p.desktop_dir.clone().unwrap();
        assert!(desktop_dir.starts_with(&xdg), "ярлыки уважают XDG_DATA_HOME: {desktop_dir}");
        let desktop = read(&format!("{desktop_dir}/{DESKTOP_FILE}"));
        assert!(desktop.contains(&format!("Exec={}\n", l.main_window())), "{desktop}");
        assert!(io.is_file(&format!("{desktop_dir}/{RECOVER_DESKTOP_FILE}")));

        // Icons and pixmap — by the common rule.
        for rel in ["icons/hicolor/16x16/apps/rexon.png", "icons/hicolor/128x128/apps/rexon.png", "icons/hicolor/scalable/apps/rexon.svg", "pixmaps/rexon.png"] {
            assert!(io.is_file(&format!("{xdg}/{rel}")), "{rel}");
        }

        // Wallpaper add-ons into the user directories.
        let gnome = format!("{}/{}", p.gnome_ext_dir.clone().unwrap(), inst::EXT_ID);
        let cinnamon = format!("{}/{}", p.cinnamon_ext_dir.clone().unwrap(), inst::EXT_ID);
        assert!(io.is_file(&format!("{gnome}/extension.js")) && io.is_file(&format!("{cinnamon}/extension.js")));

        // Fix 8: this mode has the backups folder too.
        assert!(io.is_dir(&format!("{}/.config/rexon", io.home())));
        assert!(io.is_dir(&format!("{xdg}/rexon/backups")));

        // External commands — only caches and schemas, no password.
        let icon_root = p.icon_root.clone().unwrap();
        let s = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        assert_eq!(
            io.runs(),
            vec![
                s(&["update-desktop-database", &desktop_dir]),
                s(&["gtk-update-icon-cache", "-f", "-t", &icon_root]),
                s(&["kbuildsycoca6", "--noincremental"]),
                s(&[GLIB_COMPILE_SCHEMAS, &format!("{gnome}/schemas")]),
            ]
        );
        no_stage_left(&io);

        // Installed → uninstall finds everything, including the wallpaper host and
        // the splash (fix G8). Outside its list — only the profile folders.
        let removable = inst::uninstall_user_paths(&p, |x| std::fs::symlink_metadata(x).is_ok());
        for extra in l.extra_bin_paths() {
            assert!(removable.contains(&extra), "удаление не знает {extra}");
        }
        let uncovered: Vec<String> = tree(&io.home())
            .into_iter()
            .filter(|f| !removable.iter().any(|r| f == r || f.starts_with(&format!("{r}/"))))
            .collect();
        assert_eq!(uncovered, Vec::<String>::new());
        assert!(inst::system_or_user_installed(&p, |x| io.exists(x), |x| io.is_dir(x)), "первый запуск увидит установку");

        for (stage, m) in sink.lines.lock().unwrap().iter() {
            for bad in ["pkexec", "polkit", ".desktop", "PySide", "/usr", "~/.local", "$PATH", "User-", "Portable"] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    #[test]
    fn user_reinstall_replaces_previous_copy_whole() {
        let io = ScratchIo::new();
        let l = io.layout();
        let stale = format!("{}/stale.py", l.package_dir());
        io.put(&stale[io.base.len() + 1..], "old", 0o644);
        let (r, _) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert!(!io.exists(&stale), "прежняя копия заменена целиком");
        assert!(io.is_file(&format!("{}/themes.json", l.package_dir())));
        no_stage_left(&io);
    }

    #[test]
    fn user_missing_cache_programs_are_not_a_failure() {
        let mut io = ScratchIo::new();
        io.tools.clear();
        let (r, sink) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert!(io.runs().is_empty(), "запускать нечего — и не запускали");
        let lines = sink.lines.lock().unwrap().clone();
        assert!(lines.contains(&(de_progress::STAGE_WARNING.to_string(), inst::menu_refresh_later_notice(RU))));
    }

    #[test]
    fn user_command_dir_outside_path_is_reported() {
        let mut io = ScratchIo::new();
        io.env.insert("PATH".into(), "/usr/bin".into());
        let (r, _) = user_install(&io);
        let bin = io.layout().bin_dir;
        let outcome = r.outcome.clone().unwrap();
        assert!(
            matches!(&outcome, WizardOutcome::Installed { command_dir_not_in_path: Some(d), .. } if *d == bin),
            "{outcome:?}"
        );
        assert!(inst::result_texts(&outcome, RU).notes.contains(&inst::command_path_notice(RU)));

        // A trailing slash in a PATH element — still "included".
        let mut io = ScratchIo::new();
        let bin = io.layout().bin_dir;
        io.env.insert("PATH".into(), format!("/usr/bin:{bin}/"));
        let (r, _) = user_install(&io);
        assert!(matches!(r.outcome, Some(WizardOutcome::Installed { command_dir_not_in_path: None, .. })));
    }

    #[test]
    fn user_copy_failure_keeps_previous_copy_cleans_up_and_names_step() {
        let mut io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        io.fail_copy_tree_into = Some(QML_MODULES_DIR_NAME.into());
        let (r, sink) = user_install(&io);
        let f = user_failure(&r);
        assert_eq!(f, InstallFailure::StepFailed { step: InstallStep::CopyFiles, detail: "диск переполнен".into() });
        assert_eq!(r.message, f.log_line(RU));
        assert_eq!(read(&old), "old", "починка 9: прежняя копия цела");
        assert!(!io.exists(&l.launcher()));
        assert!(io.runs().is_empty());
        assert_eq!(sink.lines.lock().unwrap().last().unwrap().0, de_progress::STAGE_ERROR);
        no_stage_left(&io);

        // A copy swap failure — the previous one goes back in place.
        let io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        *io.fail_rename_once_into.borrow_mut() = Some(l.lib_dir.clone());
        let (r, _) = user_install(&io);
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert_eq!(read(&old), "old", "прежняя копия возвращена");
        no_stage_left(&io);
    }

    #[test]
    fn user_cancel_before_swap_changes_nothing() {
        let io = ScratchIo::new();
        let l = io.layout();
        let old = format!("{}/app.py", l.package_dir());
        io.put(&old[io.base.len() + 1..], "old", 0o644);
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = Arc::new(RecSink::default());
        let r = install_for_user(&io, &io.request(), sink.clone(), &cancel);
        assert!(r.cancelled() && !r.ok(), "{r:?}");
        assert_eq!(read(&old), "old");
        assert!(!io.exists(&l.launcher()) && !io.exists(&l.main_window()));
        assert!(io.runs().is_empty());
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
        no_stage_left(&io);
    }

    #[test]
    fn user_links_are_replaced_never_written_through() {
        // The install directory is a link into the tree: not replaced at all.
        let io = ScratchIo::new();
        io.put("devtree/rexon/app.py", "dev", 0o644);
        let l = io.layout();
        std::fs::create_dir_all(parent_dir(&l.lib_dir)).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree", io.base), &l.lib_dir).unwrap();
        let (r, _) = user_install(&io);
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(std::fs::symlink_metadata(&l.lib_dir).unwrap().file_type().is_symlink());
        assert_eq!(read(&format!("{}/devtree/rexon/app.py", io.base)), "dev");
        assert!(tree(&format!("{}/devtree", io.base)).len() == 1, "в дерево ничего не легло");
        no_stage_left(&io);

        // The launcher and the wallpaper host are links: replaced themselves, the target untouched.
        let io = ScratchIo::new();
        io.put("devtree/launcher", "dev", 0o755);
        io.put("devtree/host", "dev", 0o755);
        let l = io.layout();
        std::fs::create_dir_all(&l.bin_dir).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree/launcher", io.base), l.launcher()).unwrap();
        std::os::unix::fs::symlink(format!("{}/devtree/host", io.base), l.wallpaper_host()).unwrap();
        let (r, _) = user_install(&io);
        assert!(r.ok(), "{r:?}");
        assert_eq!(read(&format!("{}/devtree/launcher", io.base)), "dev");
        assert_eq!(read(&format!("{}/devtree/host", io.base)), "dev");
        assert!(!std::fs::symlink_metadata(l.launcher()).unwrap().file_type().is_symlink());
        assert_eq!(read(&l.wallpaper_host()), "ELF");

        // A link higher up the path (Bazzite: /home → /var/home) is no reason to refuse.
        io.put("realhome/x/y", "", 0o644);
        std::os::unix::fs::symlink(format!("{}/realhome", io.base), format!("{}/linkhome", io.base)).unwrap();
        assert!(!is_redirected(&io, &format!("{}/linkhome/x", io.base)));
        assert!(is_redirected(&io, &format!("{}/linkhome", io.base)));
    }

    #[test]
    fn user_without_home_refuses_before_any_side_effect() {
        let mut io = ScratchIo::new();
        io.env.insert("HOME".into(), String::new());
        let sink = Arc::new(RecSink::default());
        let req = UserInstallRequest::new(payload(), RU);
        let r = install_for_user(&io, &req, sink, &CancelToken::new());
        assert!(matches!(user_failure(&r), InstallFailure::StepFailed { step: InstallStep::CopyFiles, .. }));
        assert!(io.runs().is_empty());
    }

    #[test]
    fn portable_prepares_profile_folders_only() {
        let mut io = ScratchIo::new();
        // Fix 8: an empty XDG_DATA_HOME means "not set", not the current directory.
        io.env.insert("XDG_DATA_HOME".into(), String::new());
        let req = InstallRequest::new(payload(), PkgFamily::Arch, "cachyos", RU);
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::Portable, &req, sink.clone(), &CancelToken::new());
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed { mode: InstallMode::Portable, fell_back: None, command_dir_not_in_path: None })
        );
        assert_eq!(r.message, inst::portable_no_install_notice(RU));
        assert_eq!(r.entry, None);
        let home = io.home();
        assert!(io.is_dir(&format!("{home}/.config/rexon")));
        assert!(io.is_dir(&format!("{home}/.local/share/rexon/backups")));
        assert!(!io.exists(&format!("{home}/.local/lib")) && !io.exists(&format!("{home}/.local/bin")));
        assert!(io.runs().is_empty());
        assert_eq!(profile_dirs("/h", "", ""), vec!["/h/.config/rexon".to_string(), "/h/.local/share/rexon/backups".into()]);
        assert!(profile_dirs("", "", "").is_empty() && profile_dirs("rel", "", "").is_empty());
    }

    #[test]
    fn dispatcher_on_immutable_system_installs_for_user_before_touching_system() {
        let io = ScratchIo::new();
        let payload = PayloadSource::from_bundle(&io, &format!("{}/src", io.base));
        let req = InstallRequest::new(payload, PkgFamily::Arch, "steamos", RU);
        let sink = Arc::new(RecSink::default());
        let r = super::install(&io, InstallMode::System, &req, sink.clone(), &CancelToken::new());
        let reason = inst::system_mode_blocked_reason_in("steamos", RU).unwrap();
        assert_eq!(Some(reason.clone()), inst::system_mode_blocked_reason("steamos"), "то же решение, что у первого запуска");
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            r.outcome,
            Some(WizardOutcome::Installed {
                mode: InstallMode::User,
                fell_back: Some(reason.clone()),
                command_dir_not_in_path: None,
            })
        );
        assert!(io.runs().iter().all(|a| a[0] != "pkexec"), "пароль не спрашивали");
        assert!(sink.has(&reason));
        assert!(inst::result_texts(r.outcome.as_ref().unwrap(), RU).notes.contains(&reason));
        assert!(io.is_executable(&io.layout().main_window()));
        no_stage_left(&io);
    }

    #[test]
    fn dispatcher_routes_system_and_user_modes() {
        let io = FakeIo::ok();
        let r = super::install(&io, InstallMode::System, &request(), Arc::new(RecSink::default()), &CancelToken::new());
        assert!(r.ok(), "{r:?}");
        assert_eq!(io.runs().len(), 1, "обычная система — установка в систему под паролем");

        let io = FakeIo::ok();
        super::install(&io, InstallMode::User, &request(), Arc::new(RecSink::default()), &CancelToken::new());
        assert!(io.runs().is_empty(), "в домашнюю папку — без пароля");
        for c in io.calls() {
            if let Call::Write(p, ..) | Call::CopyFile(_, p, _) | Call::CopyTree(_, p) | Call::MakeDirs(p, _) = &c {
                assert!(p.starts_with("/scratch/home/"), "запись вне домашней папки: {c:?}");
            }
        }
    }

    // ── Uninstall (G8) ──────────────────────────────────────────────────
    //
    // Install for everyone — the substitute `FakeIo` (not a single write and not
    // a single run); the home folder — `ScratchIo` (the real FS only inside the
    // test's temporary directory). `UninstallRequest::detect` is not called on
    // `ScratchIo`: it would look at the live `/usr/local`.

    const SYS_LIB: &str = "/usr/local/lib/rexon";
    const CONFIG: &str = "/scratch/home/.config/rexon";
    const BACKUPS: &str = "/scratch/home/.local/share/rexon/backups";

    /// An install for everyone was found, the profile is in place.
    fn with_system(mut io: FakeIo) -> FakeIo {
        io.files.insert("/usr/local/bin/rexon".into());
        io.files.insert("/usr/share/applications/rexon.desktop".into());
        io.dirs.insert(SYS_LIB.into());
        io.dirs.insert(CONFIG.into());
        io.dirs.insert(BACKUPS.into());
        io
    }

    fn removed_ok() -> RunOut {
        out(0, &marks(&[inst::UNINSTALL_SCRIPT_MARK]), "")
    }

    fn un<IO: InstallIo>(io: &IO, req: &UninstallRequest) -> (UninstallResult, Arc<RecSink>) {
        let sink = Arc::new(RecSink::default());
        let r = uninstall(io, req, sink.clone(), &CancelToken::new());
        (r, sink)
    }

    fn uninstall_script(io: &FakeIo) -> String {
        io.written(&format!("{STAGE}/uninstall.sh")).expect("скрипт удаления записан").0
    }

    fn removed(io: &FakeIo) -> Vec<String> {
        io.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::RemoveTree(p) | Call::RemoveFile(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn system_uninstall_is_one_script_under_password_then_profile_and_cleanup() {
        let io = with_system(FakeIo::new(removed_ok));
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert_eq!((req.system, req.user), (true, false));
        let (r, sink) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![] }, "{}", r.message);
        assert!(r.ok());
        assert_eq!(r.message, inst::uninstall_done_line(false, RU));

        let script = format!("{STAGE}/uninstall.sh");
        assert_eq!(io.runs(), vec![(inst::uninstall_system_cmd(&script).argv, Some(300))]);
        let text = uninstall_script(&io);
        let sys = inst::install_paths(InstallMode::System, "", None);
        assert_eq!(text, inst::uninstall_system_script(&sys), "ссылок нет — скрипт целиком из ядра");
        let layout = InstallLayout::system();
        // The wallpaper host and the splash (fix 15), the "no password" rule, the program folder.
        for must in [
            layout.wallpaper_host(),
            layout.splash_overlay(),
            plan::REXON_POLKIT_RULES_PATH.to_string(),
            SYS_LIB.to_string(),
        ] {
            assert!(text.lines().any(|l| l == format!("rm -rf {must}")), "скрипт не снимает {must}:\n{text}");
        }

        // Cleanup of the temporary folder — after the run; the profile — last.
        let calls = io.calls();
        let run_at = calls.iter().position(|c| matches!(c, Call::Run(a, _) if a[0] == "pkexec")).unwrap();
        let clean_at = calls.iter().position(|c| *c == Call::RemoveTree(STAGE.into())).unwrap();
        assert!(clean_at > run_at);
        assert_eq!(removed(&io), vec![STAGE.to_string(), CONFIG.into(), BACKUPS.into()]);

        for (stage, m) in sink.lines.lock().unwrap().iter() {
            for bad in ["pkexec", "polkit", "system", "/usr", "код "] {
                assert!(!m.contains(bad), "жаргон «{bad}» в строке хода [{stage}] «{m}»");
            }
        }
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    /// Rexon's protective files (confined autostart copies, the labwc portals
    /// file) stay with EITHER choice: the window names them, and neither the
    /// root script nor the home cleanup touches them. The person's own files of
    /// the same names are not named; our per-desktop mimeapps list is not a
    /// kept fix — it goes with Rexon, as before.
    #[test]
    fn protective_files_are_named_and_never_removed() {
        const CFG: &str = "/scratch/home/.config";
        let kept = [
            format!("{CFG}/autostart/org.fcitx.Fcitx5.desktop"),
            format!("{CFG}/xdg-desktop-portal/labwc-portals.conf"),
        ];
        let our_list = format!("{CFG}/x-cinnamon-mimeapps.list");
        let fill = |mut io: FakeIo| {
            let texts = [
                (kept[0].clone(), format!("[Desktop Entry]\nExec=fcitx5\nOnlyShowIn=Deepin;DDE;\n{}\n", crate::appdefaults::CONFINED_AUTOSTART_MARK)),
                (kept[1].clone(), format!("{}\n[preferred]\ndefault=wlr;*\n", crate::appdefaults::PORTALS_MARK)),
                (our_list.clone(), format!("{}\n[Default Applications]\n", crate::defaults::MIMEAPPS_MARK)),
                // The person's own: no mark — not ours to name.
                (format!("{CFG}/autostart/fcitx5-helper.desktop"), "[Desktop Entry]\nExec=x\n".to_string()),
                (format!("{CFG}/xdg-desktop-portal/portals.conf"), "[preferred]\ndefault=gtk\n".to_string()),
            ];
            io.texts.extend(texts);
            io
        };
        for keep in [false, true] {
            let io = fill(with_system(FakeIo::new(removed_ok)));
            let req = UninstallRequest::detect(&io, None, keep, RU);
            let listing = uninstall_files(&io, &req);
            assert_eq!(listing.kept_fixes, kept.to_vec());
            let gone = listing.removed(keep);
            for k in &kept {
                assert!(gone.iter().all(|p| !k.starts_with(p.as_str())), "{k} is in the removal list: {gone:?}");
            }
            let (r, _) = un(&io, &req);
            assert!(r.ok(), "{r:?}");
            let script = uninstall_script(&io);
            for k in &kept {
                assert!(removed(&io).iter().all(|p| !k.starts_with(p.as_str())), "{k} was removed: {:?}", removed(&io));
                assert!(!script.contains(k.as_str()) && !script.contains(CFG), "the script touches {k}:\n{script}");
            }
            assert!(removed(&io).contains(&our_list), "{:?}", removed(&io));
        }
    }

    /// Wehrwolfmann's decision of 15.09.2026: «keep» means settings AND backups.
    /// The old test asserted the opposite (settings are wiped, backups stay) —
    /// exactly the behaviour that was revoked.
    #[test]
    fn keep_profile_leaves_settings_and_backups_untouched() {
        let io = with_system(FakeIo::new(removed_ok));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, true, RU));
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.message, inst::uninstall_done_line(true, RU));
        let gone = removed(&io);
        assert_eq!(gone, vec![STAGE.to_string()], "убрана только временная папка: {gone:?}");
        for kept in [CONFIG, BACKUPS] {
            assert!(gone.iter().all(|p| !kept.starts_with(p.as_str())), "задета {kept}: {gone:?}");
        }
        // Since the settings aren't removed, there must be no "Removing settings…" line.
        assert!(!sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
        assert!(sink.has(&inst::UninstallPart::System.progress_line(RU)));

        // «Remove Everything» on the same machine removes both folders.
        let io = with_system(FakeIo::new(removed_ok));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert_eq!(r.message, inst::uninstall_done_line(false, RU));
        let gone = removed(&io);
        assert!(gone.contains(&CONFIG.to_string()) && gone.contains(&BACKUPS.to_string()), "{gone:?}");
        assert!(sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
    }

    /// ⛔ On the owner's machine `/usr/local/lib/rexon` is a link into the working tree.
    #[test]
    fn link_into_developer_tree_is_neither_removed_nor_followed() {
        let mut io = with_system(FakeIo::new(removed_ok));
        io.links.insert(SYS_LIB.into(), "/home/dev/rexon_v5/rexon".into());
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert!(req.system, "установка видна — тем важнее не снести ссылку");
        let listed = uninstall_files(&io, &req).removed(false);
        assert!(!listed.contains(&SYS_LIB.to_string()), "перечень обещает удалить ссылку: {listed:?}");
        assert!(listed.contains(&"/usr/local/bin/rexon".to_string()), "{listed:?}");

        let (r, _) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![SYS_LIB.to_string()] });
        assert!(r.message.contains(&inst::uninstall_kept_link_note(SYS_LIB, RU)), "{}", r.message);
        let text = uninstall_script(&io);
        assert!(!text.lines().any(|l| l == format!("rm -rf {SYS_LIB}")), "{text}");
        assert!(text.contains("rm -rf /usr/local/lib/rexon-system-helper\n"), "{text}");
        assert!(text.contains(&format!("rm -rf {}\n", InstallLayout::system().wallpaper_host())), "{text}");
        for p in removed(&io) {
            assert!(!p.starts_with("/home/dev") && p != SYS_LIB, "задет {p}");
        }
    }

    #[test]
    fn denied_password_stops_removal_cleans_up_and_keeps_profile() {
        let io = with_system(FakeIo::new(|| out(126, "", "Error executing command as another user: Not authorized")));
        let (r, sink) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert_eq!(r.outcome, UninstallOutcome::Failed(InstallFailure::PasswordDenied));
        assert_eq!(r.message, inst::uninstall_failure_line(&InstallFailure::PasswordDenied, RU));
        assert_eq!(removed(&io), vec![STAGE.to_string()], "убрана только временная папка");
        assert_eq!(sink.lines.lock().unwrap().last().unwrap().0, de_progress::STAGE_ERROR);
    }

    #[test]
    fn failed_removal_is_not_a_denied_password_and_other_outcomes_are_named() {
        // The script started (the marker is there) and failed — even with code 127 it is an uninstall failure.
        let io = with_system(FakeIo::new(|| {
            out(127, &marks(&[inst::UNINSTALL_SCRIPT_MARK]), "rm: cannot remove: Read-only file system")
        }));
        let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        let UninstallOutcome::Failed(InstallFailure::StepFailed { detail, .. }) = &r.outcome else {
            panic!("ждали сбой удаления: {r:?}")
        };
        assert!(detail.contains("Read-only file system"), "{detail}");
        assert!(r.message.contains("Read-only file system"), "{}", r.message);
        assert_eq!(removed(&io), vec![STAGE.to_string()]);

        let failing = |run: RunOut| {
            let io = with_system(FakeIo::new(move || run.clone()));
            let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
            assert_eq!(removed(&io), vec![STAGE.to_string()], "уборка на любом исходе");
            r.outcome
        };
        let blank = |outcome| RunOut { outcome, returncode: None, stdout: String::new(), stderr: String::new() };
        assert_eq!(failing(blank(RunOutcome::Error)), UninstallOutcome::Failed(InstallFailure::PasswordPromptMissing));
        assert_eq!(failing(blank(RunOutcome::Timeout)), UninstallOutcome::Failed(InstallFailure::TimedOut { secs: 300 }));
    }

    #[test]
    fn script_write_failure_is_a_failure_and_asks_no_password() {
        struct NoWrite(FakeIo);
        impl SystemIo for NoWrite {
            fn exists(&self, p: &str) -> bool { self.0.exists(p) }
            fn is_file(&self, p: &str) -> bool { self.0.is_file(p) }
            fn is_dir(&self, p: &str) -> bool { self.0.is_dir(p) }
            fn is_executable(&self, p: &str) -> bool { self.0.is_executable(p) }
            fn realpath(&self, p: &str) -> Option<String> { self.0.realpath(p) }
            fn read_text(&self, p: &str) -> Option<String> { self.0.read_text(p) }
            fn list_dir(&self, p: &str) -> Option<Vec<String>> { self.0.list_dir(p) }
            fn which(&self, n: &str) -> bool { self.0.which(n) }
            fn run(&self, a: &[String], t: Option<u64>, l: bool) -> RunOut { self.0.run(a, t, l) }
            fn env(&self, n: &str) -> String { self.0.env(n) }
            fn which_path(&self, n: &str) -> Option<String> { self.0.which_path(n) }
            fn write_text(&self, p: &str, c: &str) -> Result<(), String> { self.0.write_text(p, c) }
            fn copy_file(&self, s: &str, d: &str) -> Result<(), String> { self.0.copy_file(s, d) }
        }
        impl InstallIo for NoWrite {
            fn temp_dir(&self, p: &str, par: Option<&str>) -> Result<String, String> { self.0.temp_dir(p, par) }
            fn rename(&self, s: &str, d: &str) -> Result<(), String> { self.0.rename(s, d) }
            fn make_dirs(&self, p: &str, m: Option<u32>) -> Result<(), String> { self.0.make_dirs(p, m) }
            fn write_file_mode(&self, _p: &str, _c: &str, _m: u32) -> Result<(), String> { Err("нет места".into()) }
            fn copy_file_mode(&self, s: &str, d: &str, m: u32) -> Result<(), String> { self.0.copy_file_mode(s, d, m) }
            fn copy_tree(&self, s: &str, d: &str, sk: &dyn Fn(&str) -> bool, m: &dyn Fn(bool, bool) -> u32) -> Result<usize, String> { self.0.copy_tree(s, d, sk, m) }
            fn remove_file(&self, p: &str) { self.0.remove_file(p) }
            fn remove_tree(&self, p: &str) { self.0.remove_tree(p) }
        }
        let io = NoWrite(with_system(FakeIo::new(removed_ok)));
        let (r, _) = un(&io, &UninstallRequest::detect(&io, None, false, RU));
        assert!(matches!(&r.outcome, UninstallOutcome::Failed(InstallFailure::StepFailed { detail, .. }) if detail == "нет места"), "{r:?}");
        assert!(io.0.runs().is_empty(), "пароль не спрашивали");
        assert_eq!(removed(&io.0), vec![STAGE.to_string()], "починка 18: временная папка убрана");
    }

    #[test]
    fn cancel_before_password_touches_nothing() {
        let io = with_system(FakeIo::new(removed_ok));
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = Arc::new(RecSink::default());
        let r = uninstall(&io, &UninstallRequest::detect(&io, None, false, RU), sink.clone(), &cancel);
        assert_eq!(r.outcome, UninstallOutcome::Cancelled);
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
        assert_eq!(sink.cancellable.lock().unwrap().first(), Some(&true));
        assert_eq!(sink.cancellable.lock().unwrap().last(), Some(&false));
    }

    #[test]
    fn package_goes_first_through_its_manager_then_system() {
        let io = with_system(FakeIo::new(removed_ok));
        let pkg = DistroPackage { family: PkgFamily::Arch, name: "rexon-git".into() };
        let req = UninstallRequest::detect(&io, Some(pkg), false, RU);
        assert!(uninstall_files(&io, &req).program[0].contains("rexon-git"));
        let (r, sink) = un(&io, &req);
        assert!(r.ok(), "{r:?}");
        let argv: Vec<Vec<String>> = io.runs().into_iter().map(|(a, _)| a).collect();
        assert_eq!(argv.len(), 2, "{argv:?}");
        assert_eq!(argv[0], vec!["pkexec", "pacman", "-R", "--noconfirm", "rexon-git"]);
        assert_eq!(argv[1][..2], ["pkexec", "bash"]);
        assert!(sink.has(&inst::UninstallPart::Package.progress_line(RU)));
    }

    #[test]
    fn denied_password_for_package_stops_before_everything_else() {
        let io = with_system(FakeIo::new(|| out(126, "", "")));
        let pkg = DistroPackage { family: PkgFamily::Arch, name: "rexon".into() };
        let (r, _) = un(&io, &UninstallRequest::detect(&io, Some(pkg), false, RU));
        assert_eq!(r.outcome, UninstallOutcome::Failed(InstallFailure::PasswordDenied));
        assert_eq!(io.runs().len(), 1, "скрипт удаления не запускали");
        assert!(removed(&io).is_empty(), "{:?}", io.calls());
    }

    #[test]
    fn package_without_known_remover_is_refused_before_any_side_effect() {
        let io = with_system(FakeIo::ok());
        let pkg = DistroPackage { family: PkgFamily::Unknown, name: "rexon".into() };
        let (r, _) = un(&io, &UninstallRequest::detect(&io, Some(pkg), false, RU));
        assert_eq!(r.outcome, UninstallOutcome::PackageUnsupported { name: "rexon".into() });
        assert_eq!(r.message, inst::uninstall_package_unsupported_line("rexon", RU));
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    /// Fix 19: previously the settings were wiped and it said "not installed".
    #[test]
    fn nothing_installed_touches_nothing_not_even_settings() {
        let mut io = FakeIo::ok();
        io.dirs.insert(CONFIG.into());
        let req = UninstallRequest::detect(&io, None, false, RU);
        assert!(uninstall_files(&io, &req).is_empty());
        let (r, _) = un(&io, &req);
        assert_eq!(r.outcome, UninstallOutcome::NotInstalled);
        assert_eq!(r.message, inst::uninstall_nothing_line(RU));
        assert!(!r.ok());
        assert!(io.calls().is_empty(), "{:?}", io.calls());
    }

    #[test]
    fn foreign_plan_step_is_refused() {
        let sys = inst::install_paths(InstallMode::System, "", None);
        let mut steps = inst::uninstall_system_plan(&sys, "/tmp/x.sh");
        assert!(check_uninstall_plan(&steps));
        steps.push(PlanStep::EnsureGentooOverlay { overlay: "чужой".into() });
        assert!(!check_uninstall_plan(&steps));
        // An install run in the uninstall plan is a foreign step too.
        let install_run = PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: inst::uninstall_system_cmd("/tmp/x.sh"),
            needs_root: true,
            description: String::new(),
        };
        assert!(!check_uninstall_plan(&[install_run]));
    }

    #[test]
    fn distro_package_owner_is_asked_of_the_package_manager() {
        let mut io = FakeIo::ok();
        io.tools.insert("pacman".into());
        io.files.insert("/usr/bin/rexon".into());
        io.reply = Box::new(|argv: &[String]| {
            if argv.last().map(String::as_str) == Some("/usr/bin/rexon") {
                out(0, "rexon-git\n", "")
            } else {
                out(1, "", "error: No package owns")
            }
        });
        let got = detect_distro_package(&io, PkgFamily::Arch, Some("/opt/rexon/rexon-main-window"));
        assert_eq!(got, Some(DistroPackage { family: PkgFamily::Arch, name: "rexon-git".into() }));
        let asked: Vec<(Vec<String>, Option<u64>)> = io
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Run(a, t) => Some((a, t)),
                _ => None,
            })
            .collect();
        let q = |p: &str| (vec!["pacman".to_string(), "-Qoq".into(), p.into()], Some(OWNER_QUERY_TIMEOUT_SECS));
        assert_eq!(asked, vec![q("/opt/rexon/rexon-main-window"), q("/usr/bin/rexon")]);

        // No manager — we don't ask; the family has no detection — likewise.
        let io = FakeIo::ok();
        assert_eq!(detect_distro_package(&io, PkgFamily::Arch, Some("/x")), None);
        assert_eq!(detect_distro_package(&io, PkgFamily::Gentoo, Some("/x")), None);
        assert!(io.calls().is_empty());
    }

    fn user_request(keep_profile: bool) -> UninstallRequest {
        UninstallRequest { package: None, system: false, user: true, keep_profile, lang: RU }
    }

    #[test]
    fn user_uninstall_removes_what_install_laid_out_host_and_splash_included() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        io.put("home/.config/rexon/settings.json", "{}", 0o644);
        io.put("home/.local/share/rexon/backups/plasma-panel/1.json", "{}", 0o644);
        let l = io.layout();
        let listed = uninstall_files(&io, &user_request(false)).removed(false);
        for exe in l.extra_bin_paths() {
            assert!(io.is_file(&exe), "{exe}");
            assert!(listed.contains(&exe), "перечень не называет {exe}: {listed:?}");
        }
        let runs_before = io.runs().len();
        let (r, sink) = un(&io, &user_request(false));
        assert_eq!(r.outcome, UninstallOutcome::Removed { kept_links: vec![] }, "{}", r.message);
        assert_eq!(tree(&io.home()), Vec::<String>::new(), "в домашней папке не осталось файлов Rexon");
        assert!(!io.exists(&l.lib_dir));
        let after = io.runs()[runs_before..].to_vec();
        assert!(after.iter().all(|a| a[0] != "pkexec"), "без пароля: {after:?}");
        assert!(after.iter().any(|a| a[0] == UPDATE_DESKTOP_DATABASE), "меню обновлено: {after:?}");
        assert!(sink.has(&inst::UninstallPart::User.progress_line(RU)));
        assert!(!sink.has(&inst::menu_refresh_later_notice(RU)));
    }

    #[test]
    fn uninstall_during_aero_fallback_returns_the_look_and_removes_the_watch_unit() {
        let mut io = ScratchIo::new();
        io.tools.extend(["systemctl".to_string(), "dbus-send".to_string()]);
        assert!(user_install(&io).0.ok());
        let home = io.home();
        let paths = crate::session_guard::GuardPaths::new(format!("{home}/.config"), format!("{home}/.local/state/rexon/session-guard"));
        io.put("home/.config/kwinrc", "[Plugins]\nsmodglowEnabled=true\n", 0o644);
        let plan = crate::session_guard::FallbackPlan { effects: vec!["smodglow".into()], applets: vec![], why: vec![] };
        crate::session_guard::fall_back_to_stock(&paths, &plan, crate::session_guard::FallbackCause::Requested { reason: "x".into() }).unwrap();
        assert!(read(&format!("{home}/.config/kwinrc")).contains("smodglowEnabled=false"));
        let unit = crate::session_watch::unit_path(&format!("{home}/.config"));
        io.put(&format!("home/.config/systemd/user/{}", crate::session_watch::UNIT_NAME), "[Unit]\n", 0o644);
        io.put(&format!("home/.config/plasma-workspace/env/{}", crate::session_guard::ENV_SCRIPT_NAME), "guard", 0o644);

        let (r, _) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert!(read(&format!("{home}/.config/kwinrc")).contains("smodglowEnabled=true"), "эффект вернулся");
        assert!(!io.exists(&unit), "юнит сторожа сеанса снят");
        assert!(!paths.env_script().exists(), "раскладка не подменялась — сценарию незачем оставаться");
        let runs = io.runs();
        let sc = |a: &[&str]| runs.iter().any(|r| r.iter().map(String::as_str).eq(a.iter().copied()));
        assert!(sc(&["systemctl", "--user", "disable", crate::session_watch::UNIT_NAME]), "{runs:?}");
        assert!(sc(&["systemctl", "--user", "daemon-reload"]), "{runs:?}");
        assert!(runs.iter().any(|r| r == &crate::session_guard::reconfigure_kwin_argv()), "KWin перечитал настройки: {runs:?}");
    }

    #[test]
    fn uninstall_removes_our_screensaver_launcher_copies_only() {
        use crate::screensaver_guard as sg;
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        let ours = "home/.local/share/applications/xscreensaver-settings.desktop";
        let mine = "home/.local/share/applications/xscreensaver.desktop";
        io.put(ours, &format!("[Desktop Entry]\n{}\nExec=xscreensaver-settings\n", sg::MARKER_LINE), 0o644);
        io.put(mine, "[Desktop Entry]\nExec=xscreensaver\n", 0o644);
        let (r, _) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert!(!io.exists(&format!("{}/{ours}", io.base)), "our copy is removed");
        assert!(io.exists(&format!("{}/{mine}", io.base)), "the person's own copy stays");
    }

    #[test]
    fn uninstall_removes_our_notifications_slot_and_keeps_foreign_one() {
        use crate::notifications_slot as ns;
        let rel = "home/.local/share/dbus-1/services/org.freedesktop.Notifications.service";
        let busctl = crate::appdefaults::busctl_reload_cmd();

        // Our file (the marker in the first line) — goes away, the bus rereads.
        let mut io = ScratchIo::new();
        io.tools.insert("busctl".to_string());
        assert!(user_install(&io).0.ok());
        let path = format!("{}/{rel}", io.base);
        io.put(rel, &format!("{}\n[D-BUS Service]\nName={}\nExec=/bin/false\n", ns::MARKER, ns::BUS_NAME), 0o644);
        let (r, _) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert!(!io.exists(&path), "наш файл выбора демона уведомлений убран");
        assert!(io.runs().contains(&busctl), "{:?}", io.runs());

        // A foreign file — stays in place, the bus is not poked.
        let mut io = ScratchIo::new();
        io.tools.insert("busctl".to_string());
        assert!(user_install(&io).0.ok());
        let path = format!("{}/{rel}", io.base);
        let foreign = format!("[D-BUS Service]\nName={}\nExec=/usr/bin/mako\n", ns::BUS_NAME);
        io.put(rel, &foreign, 0o644);
        let (r, _) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert_eq!(read(&path), foreign, "чужой файл не тронут");
        assert!(!io.runs().contains(&busctl), "{:?}", io.runs());
    }

    #[test]
    fn user_uninstall_keeping_profile_keeps_settings_and_backups() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        io.put("home/.config/rexon/settings.json", "мои настройки", 0o644);
        io.put("home/.config/rexon/ocs_installed.json", "журнал тем", 0o644);
        io.put("home/.local/share/rexon/backups/plasma-panel/1.json", "копия", 0o644);
        let (r, sink) = un(&io, &user_request(true));
        assert!(r.ok(), "{}", r.message);
        assert_eq!(r.message, inst::uninstall_done_line(true, RU));
        let home = io.home();
        let settings = format!("{home}/.config/rexon/settings.json");
        let journal = format!("{home}/.config/rexon/ocs_installed.json");
        let copy = format!("{home}/.local/share/rexon/backups/plasma-panel/1.json");
        let mut want = vec![settings.clone(), journal.clone(), copy.clone()];
        want.sort();
        assert_eq!(tree(&home), want, "остались настройки и резервная копия, файлов программы нет");
        assert_eq!(read(&settings), "мои настройки");
        assert_eq!(read(&journal), "журнал тем");
        assert_eq!(read(&copy), "копия");
        assert!(!sink.has(&inst::UninstallPart::Profile.progress_line(RU)));
    }

    /// Profile folders are taken with `XDG_CONFIG_HOME` and `XDG_DATA_HOME`
    /// applied: «keep» leaves exactly them, «Remove Everything» removes exactly
    /// them. The literal `~/.config/rexon` is OURS too: before fix 8a45776 the
    /// settings were written there even with `XDG_CONFIG_HOME` set, so it goes
    /// along with the XDG one. A foreign neighbour next to it in `~/.config` is
    /// untouched in either case.
    #[test]
    fn profile_follows_xdg_dirs_in_both_choices() {
        for keep in [true, false] {
            let mut io = ScratchIo::new();
            io.env.insert("XDG_CONFIG_HOME".into(), format!("{}/xdg-config", io.base));
            io.env.insert("XDG_DATA_HOME".into(), format!("{}/xdg-data", io.base));
            assert!(user_install(&io).0.ok());
            io.put("xdg-config/rexon/settings.json", "мои", 0o644);
            io.put("xdg-data/rexon/backups/plasma-panel/1.json", "копия", 0o644);
            // Settings from an install made before fix 8a45776.
            io.put("home/.config/rexon/settings.json", "мои прежние", 0o644);
            io.put("home/.config/not-ours.json", "чужое место", 0o644);
            let config = format!("{}/xdg-config/rexon", io.base);
            let old_config = format!("{}/home/.config/rexon", io.base);
            let backups = format!("{}/xdg-data/rexon/backups", io.base);
            let listed = uninstall_files(&io, &user_request(keep)).removed(false);
            assert!(
                listed.contains(&config) && listed.contains(&old_config) && listed.contains(&backups),
                "{listed:?}"
            );

            let (r, _) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            assert_eq!(io.exists(&format!("{config}/settings.json")), keep, "настройки, keep={keep}");
            assert_eq!(
                io.exists(&format!("{old_config}/settings.json")),
                keep,
                "прежние настройки, keep={keep}"
            );
            assert_eq!(io.exists(&format!("{backups}/plasma-panel/1.json")), keep, "копии, keep={keep}");
            assert_eq!(read(&format!("{}/home/.config/not-ours.json", io.base)), "чужое место");
            assert!(!io.exists(&io.layout().lib_dir), "программа удалена, keep={keep}");
        }
    }

    /// Rexon data of a person: cache (XDG and literal), settings, backups and
    /// installed content, plus foreign neighbours the uninstall must not touch.
    /// Returns (what always goes, what goes only with «Remove Everything», what
    /// isn't ours).
    fn put_user_data(io: &ScratchIo, cache_root: &str, data_root: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let b = &io.base;
        let files = [
            (format!("{cache_root}/rexon/youtube/abc.mp4"), true),
            ("home/.cache/rexon/ocs_cache.db".to_string(), true),
            ("home/.config/rexon/settings.json".to_string(), false),
            (format!("{data_root}/rexon/backups/plasma-panel/1.json"), false),
            (format!("{data_root}/rexon/wallpapers/волны/scene.json"), false),
            (format!("{data_root}/rexon/login-themes/тема/Main.qml"), false),
            (format!("{data_root}/rexon/plymouth-themes/p/p.plymouth"), false),
            (format!("{data_root}/rexon/bundles/b.rxn"), false),
            (format!("{data_root}/rexon/extensions/e/metadata.json"), false),
            (format!("{data_root}/rexon/qml_modules/Qcm/qmldir"), false),
            ("home/.local/share/rexon/bin/realesrgan/realesrgan-ncnn-vulkan".to_string(), false),
        ];
        let (mut always, mut full) = (Vec::new(), Vec::new());
        for (rel, is_cache) in files {
            io.put(&rel, "данные", 0o644);
            if is_cache { always.push(format!("{b}/{rel}")) } else { full.push(format!("{b}/{rel}")) }
        }
        let foreign = [
            format!("{cache_root}/other-app/c"),
            "home/.cache/rexon-other/c".to_string(),
            format!("{data_root}/other-app/d"),
            "home/.local/share/applications/other.desktop".to_string(),
            "home/.config/other/o".to_string(),
        ];
        for rel in &foreign {
            io.put(rel, "чужое", 0o644);
        }
        (always, full, foreign.iter().map(|r| format!("{b}/{r}")).collect())
    }

    /// ⭐ Decision of 15.09.2026: «keep» — the cache is removed, settings, backups
    /// and installed wallpapers and themes are intact; «Remove Everything» —
    /// everything removed, the `rexon` roots in the cache and data too. Foreign
    /// stuff untouched in either case.
    #[test]
    fn cache_goes_always_installed_content_only_with_delete_all() {
        for keep in [true, false] {
            let io = ScratchIo::new();
            assert!(user_install(&io).0.ok());
            let (always, full, foreign) = put_user_data(&io, "home/.cache", "home/.local/share");
            let home = io.home();

            let listing = uninstall_files(&io, &user_request(keep));
            use inst::UserDataKind as K;
            let kinds_of = |k: K| -> Vec<String> {
                listing.user_data.iter().filter(|(kind, _)| *kind == k).map(|(_, p)| p.clone()).collect()
            };
            assert_eq!(kinds_of(K::Cache), vec![format!("{home}/.cache/rexon")]);
            assert_eq!(kinds_of(K::Settings), vec![format!("{home}/.config/rexon")]);
            assert_eq!(kinds_of(K::Backups), vec![format!("{home}/.local/share/rexon/backups")]);
            let content: Vec<String> = ["bin", "bundles", "extensions", "login-themes", "plymouth-themes", "qml_modules", "wallpapers"]
                .iter()
                .map(|n| format!("{home}/.local/share/rexon/{n}"))
                .collect();
            assert_eq!(kinds_of(K::Content), content, "содержимое без резервных копий, по имени");
            let removed = listing.removed(keep);
            assert!(removed.contains(&format!("{home}/.cache/rexon")), "кэш уходит при любом выборе");
            assert_eq!(removed.iter().any(|p| p.contains("/.local/share/rexon")), !keep, "{removed:?}");
            assert_eq!(removed.iter().any(|p| p.contains("/.config/rexon")), !keep, "{removed:?}");

            let (r, sink) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            assert_eq!(r.message, inst::uninstall_done_line(keep, RU));
            for p in &always {
                assert!(!io.exists(p), "кэш остался, keep={keep}: {p}");
            }
            assert!(!io.exists(&format!("{home}/.cache/rexon")), "корень кэша остался");
            for p in &full {
                assert_eq!(io.exists(p), keep, "keep={keep}: {p}");
            }
            for p in &foreign {
                assert_eq!(read(p), "чужое", "задето чужое: {p}");
            }
            assert_eq!(io.exists(&format!("{home}/.local/share/rexon")), keep, "корень данных, keep={keep}");
            assert!(sink.has(&inst::UninstallPart::Cache.progress_line(RU)));
            assert_eq!(sink.has(&inst::UninstallPart::Profile.progress_line(RU)), !keep);
        }
    }

    /// `XDG_CACHE_HOME` and `XDG_DATA_HOME`: the roots are taken both per XDG and
    /// literal (part of the core writes to `~/.cache/rexon` bypassing XDG).
    #[test]
    fn cache_and_content_follow_xdg_and_literal_roots() {
        for keep in [true, false] {
            let mut io = ScratchIo::new();
            io.env.insert("XDG_CACHE_HOME".into(), format!("{}/xdg-cache", io.base));
            io.env.insert("XDG_DATA_HOME".into(), format!("{}/xdg-data", io.base));
            assert!(user_install(&io).0.ok());
            let (always, full, foreign) = put_user_data(&io, "xdg-cache", "xdg-data");
            let listed = uninstall_files(&io, &user_request(keep)).removed(true);
            for root in [format!("{}/xdg-cache/rexon", io.base), format!("{}/.cache/rexon", io.home())] {
                assert!(listed.contains(&root), "{root}: {listed:?}");
            }
            let (r, _) = un(&io, &user_request(keep));
            assert!(r.ok(), "{}", r.message);
            for p in &always {
                assert!(!io.exists(p), "кэш остался, keep={keep}: {p}");
            }
            for p in &full {
                assert_eq!(io.exists(p), keep, "keep={keep}: {p}");
            }
            for p in &foreign {
                assert_eq!(read(p), "чужое", "задето чужое: {p}");
            }
            for root in ["xdg-data/rexon", "home/.local/share/rexon"] {
                assert_eq!(io.exists(&format!("{}/{root}", io.base)), keep, "корень {root}, keep={keep}");
            }
        }
    }

    /// ⛔ Links: in place of the cache, installed content and the whole data root
    /// the link ITSELF is removed; what it points to is intact. A link to the
    /// backups — as before, neither removed nor traversed.
    #[test]
    fn links_in_cache_and_content_are_removed_themselves_never_followed() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        let b = io.base.clone();
        let home = io.home();
        for (rel, text) in [
            ("elsewhere/cache/big.bin", "кэш на другом диске"),
            ("elsewhere/walls/мои.jpg", "обои на другом диске"),
            ("elsewhere/photo.jpg", "фото"),
            ("elsewhere/backups/1.json", "копии на другом диске"),
            ("home/.local/share/rexon/login-themes/t/Main.qml", "тема"),
        ] {
            io.put(rel, text, 0o644);
        }
        let link = |target: &str, at: &str| {
            std::fs::create_dir_all(parent_dir(at)).unwrap();
            // The install already created an empty backups folder — the link takes its place.
            let _ = std::fs::remove_dir(at);
            std::os::unix::fs::symlink(format!("{b}/{target}"), at).unwrap()
        };
        let cache = format!("{home}/.cache/rexon");
        let walls = format!("{home}/.local/share/rexon/wallpapers");
        let inner = format!("{home}/.local/share/rexon/login-themes/t/фон.jpg");
        let backups = format!("{home}/.local/share/rexon/backups");
        link("elsewhere/cache", &cache);
        link("elsewhere/walls", &walls);
        link("elsewhere/photo.jpg", &inner);
        link("elsewhere/backups", &backups);

        let listing = uninstall_files(&io, &user_request(false));
        let listed = listing.removed(false);
        assert!(listed.contains(&cache) && listed.contains(&walls), "удалится сама ссылка — назвать: {listed:?}");
        assert!(!listed.contains(&backups), "ссылку на копии не удаляем — не называть: {listed:?}");

        let (r, _) = un(&io, &user_request(false));
        let UninstallOutcome::Removed { kept_links } = &r.outcome else { panic!("{r:?}") };
        assert_eq!(kept_links, &vec![backups.clone()]);
        for gone in [&cache, &walls] {
            assert!(std::fs::symlink_metadata(gone).is_err(), "ссылка {gone} осталась");
            assert!(r.message.contains(&inst::uninstall_removed_link_note(gone, RU)), "{}", r.message);
        }
        assert!(std::fs::symlink_metadata(&inner).is_err(), "каталог с ссылкой внутри удалён");
        assert!(std::fs::symlink_metadata(&backups).unwrap().file_type().is_symlink());
        assert_eq!(read(&format!("{b}/elsewhere/cache/big.bin")), "кэш на другом диске");
        assert_eq!(read(&format!("{b}/elsewhere/walls/мои.jpg")), "обои на другом диске");
        assert_eq!(read(&format!("{b}/elsewhere/photo.jpg")), "фото");
        assert_eq!(read(&format!("{b}/elsewhere/backups/1.json")), "копии на другом диске");
        // The data root is not empty (a link to the backups remained) — untouched.
        assert!(io.is_dir(&format!("{home}/.local/share/rexon")));
    }

    /// The whole data root is a link: only it is removed, the backups and
    /// wallpapers on the far side are intact and are not named one by one.
    #[test]
    fn linked_data_root_is_unlinked_and_its_backups_are_not_entered() {
        let io = ScratchIo::new();
        assert!(user_install(&io).0.ok());
        let b = io.base.clone();
        io.put("dotfiles/rexon-data/backups/1.json", "копия", 0o644);
        io.put("dotfiles/rexon-data/wallpapers/w.jpg", "обои", 0o644);
        let root = format!("{}/.local/share/rexon", io.home());
        // The install created a backups folder there — the link takes the root's place.
        std::fs::remove_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(format!("{b}/dotfiles/rexon-data"), &root).unwrap();

        let listing = uninstall_files(&io, &user_request(false));
        let data: Vec<&String> = listing
            .user_data
            .iter()
            .filter(|(k, _)| matches!(k, inst::UserDataKind::Content | inst::UserDataKind::Backups))
            .map(|(_, p)| p)
            .collect();
        assert_eq!(data, vec![&root], "одна строка — сама ссылка, копии по ту сторону не названы");
        for keep in [true, false] {
            if keep {
                let (r, _) = un(&io, &user_request(true));
                assert!(r.ok() && std::fs::symlink_metadata(&root).is_ok(), "«сохранить» не трогает ссылку");
                continue;
            }
            let (r, _) = un(&io, &user_request(false));
            assert!(r.ok(), "{}", r.message);
            assert!(std::fs::symlink_metadata(&root).is_err(), "ссылка удалена");
            assert_eq!(read(&format!("{b}/dotfiles/rexon-data/backups/1.json")), "копия");
            assert_eq!(read(&format!("{b}/dotfiles/rexon-data/wallpapers/w.jpg")), "обои");
        }
    }

    #[test]
    fn user_uninstall_never_removes_or_follows_links_into_developer_tree() {
        let io = ScratchIo::new();
        io.put("devtree/rexon/app.py", "dev", 0o644);
        io.put("devtree/target/rexon-wallpaper-host", "ELF", 0o755);
        io.put("dotfiles/rexon/settings.json", "мои", 0o644);
        io.put("home/.local/bin/rexon", "#!/bin/sh", 0o755);
        io.put("home/.local/share/applications/rexon.desktop", "[Desktop Entry]", 0o644);
        let l = io.layout();
        let home = io.home();
        let config = format!("{home}/.config/rexon");
        std::fs::create_dir_all(parent_dir(&l.lib_dir)).unwrap();
        std::fs::create_dir_all(parent_dir(&config)).unwrap();
        let link = |target: &str, at: &str| std::os::unix::fs::symlink(format!("{}/{target}", io.base), at).unwrap();
        link("devtree", &l.lib_dir);
        link("devtree/target/rexon-wallpaper-host", &l.wallpaper_host());
        link("dotfiles/rexon", &config);

        let req = user_request(false);
        let listed = uninstall_files(&io, &req).removed(false);
        for k in [&l.lib_dir, &l.wallpaper_host(), &config] {
            assert!(!listed.contains(k), "перечень обещает удалить ссылку {k}: {listed:?}");
        }
        let (r, _) = un(&io, &req);
        let UninstallOutcome::Removed { kept_links } = &r.outcome else { panic!("{r:?}") };
        let mut kept = kept_links.clone();
        kept.sort();
        let mut want = vec![l.lib_dir.clone(), l.wallpaper_host(), config.clone()];
        want.sort();
        assert_eq!(kept, want);
        for k in &want {
            assert!(std::fs::symlink_metadata(k).unwrap().file_type().is_symlink(), "ссылка {k} на месте");
            assert!(r.message.contains(&inst::uninstall_kept_link_note(k, RU)), "{}", r.message);
        }
        assert_eq!(read(&format!("{}/devtree/rexon/app.py", io.base)), "dev");
        assert_eq!(read(&format!("{}/devtree/target/rexon-wallpaper-host", io.base)), "ELF");
        assert_eq!(read(&format!("{}/dotfiles/rexon/settings.json", io.base)), "мои");
        assert!(!io.exists(&l.launcher()), "обычный файл удалён");
    }

    /// ⛔ A live executor is never created in tests: a test with a real password
    /// would install Rexon into the system (memory: on 15.07 a test with a
    /// substitute HOME silently replaced the production symlink `/usr/local/lib/rexon`).
    #[test]
    fn tests_never_build_live_io() {
        let whole = include_str!("installer_exec.rs");
        let tests = whole.split_once("#[cfg(test)]").map(|(_, t)| t).unwrap();
        for forbidden in [
            ["Live", "InstallIo::", "new"].concat(),
            ["Live", "InstallIo::", "default"].concat(),
            ["Live", "DeOpsIo"].concat(),
            ["Live", "SystemIo"].concat(),
        ] {
            assert!(!tests.contains(&forbidden), "{forbidden}");
        }
    }
}
