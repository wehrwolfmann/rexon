//! Where the built QML plugins and the tabs' Rust builds lie — port of
//! `rexon/qt_wallpaper_engine/qml_ui/__init__.py`, parts of `qml_ui/bridge.py`
//! and `rexon/qt_main/__init__.py`.
//!
//! The "Wallpaper Engine" tab shows the vendored waywallen QML, and the types of
//! this QML are provided by three separate builds: `Qcm.Material` (C++, MIT),
//! `waywallen.control` (Qt-Protobuf value types) and our crate
//! `rexon-qt-filter` (the `rexon-gui` types are statically embedded into the
//! window itself, it has no separate `.so`). The fourth one is optional: the
//! author's real `QExtra` plugin (C++); without it our QML substitute works, the
//! choice is made by [`choose_qextra`]. All of them are built outside the wheel
//! and laid out into directories that depend on where Rexon was installed from:
//! the developer tree, a user build by the `tools/build_qml_*.sh` scripts, or a
//! distribution package. The question "where exactly" was the only DECISION
//! left in this layer in Python.
//!
//! There were THREE copies of this decision, and they had already drifted apart
//! (rule #12): `qml_ui.rexon_gui_candidates`,
//! `bridge._rust_filter_model_candidates` and `qt_main.rust_gui_candidates`. Now
//! there is one list; Python stays the executor — `os.path.isfile`, `glob` and
//! `ctypes.CDLL`.
//!
//! Warning — fixes to the original (rule #16), each marked `Fix:` in place:
//!   1. `qt_main.rust_gui_candidates` knew neither `$XDG_DATA_DIRS`, nor
//!      `/usr/lib64`, nor the multiarch directory `/usr/lib/<triplet>` — i.e. it
//!      carried exactly the defect of searching only in `/usr/lib` that had
//!      already been fixed in the second copy. On Fedora, openSUSE, Debian and
//!      Ubuntu the packaged `librexon_gui.so` was not found, the `org.rexon.gui`
//!      QML module did not come up — and ALL ported tabs fell back to widgets,
//!      not just the wallpaper tab;
//!   2. our crates' `.so` were searched for in only ONE module directory — the
//!      one where `Qcm.Material` was found. If `Qcm.Material` was not built, the
//!      directory collapsed to the first candidate (`~/.local/share/…`), and the
//!      build lying in `/usr/lib/rexon/qml_modules/waywallen/control/librexon_gui.so`
//!      was not checked at all: the daemon transport was unreachable with a live
//!      file on disk. Now every file is searched for across the WHOLE list;
//!   3. the user module directory was taken literally from `~/.local/share`,
//!      bypassing `$XDG_DATA_HOME` — the same class as the fixed
//!      `$XDG_CONFIG_HOME` of the autostart unit in `we_client`. Now it is
//!      respected too, and `~/.local/share` remains the second candidate:
//!      `tools/build_qml_*.sh` put the build exactly there.
//!
//! The check is `rust/rexon-core/tests/qml_modules_parity.rs`.

use crate::utils::data_home;

// ============================================================================
// Live file system checks
// ============================================================================

/// File system checks for finding builds. The core does not go into the file
/// system itself — the same technique as [`crate::we_client::DaemonFs`]: the
/// order inside, the side effect outside.
pub trait ModuleFs {
    /// `os.path.isfile(path)` — an existing regular file.
    fn is_file(&mut self, path: &str) -> bool;

    /// Which Qt version (major, minor) the plugin at this path was built for —
    /// [`plugin_qt_version`] parsing over the file's contents. `None` — cannot be
    /// read or not a QML plugin: then the version check does not interfere.
    fn plugin_qt_version(&mut self, _path: &str) -> Option<(u32, u32)> {
        None
    }

    /// The Qt version (major, minor) the process is running on now. `None` —
    /// unknown, the version check does not interfere.
    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        None
    }

    /// Contents of a small text file (the module's build mark,
    /// [`CONTROL_STAMP_REL`]). `None` — there is no file or it could not be read.
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
}

// ============================================================================
// The Qt version a plugin was built for
// ============================================================================
//
// Warning — why (caught on 09.10.2026). `Qcm.Material` uses PRIVATE Qt Quick
// API (`QuickPrivate`), and private API is not compatible between minor Qt
// releases. Qt itself does not check this: a plugin built for 6.11 happily loads
// into Qt 6.12 — and corrupts the scene's node tree. The wallpaper tab crashed
// with SIGSEGV in `QSGNode::removeChildNode` ← `updateDirtyNode` when opening
// "Settings" (the `Qcm.Material` input fields are built there), in about half
// of the launches. A plugin of the same revision rebuilt for 6.12 never crashed.
// So a `Qcm.Material` build for another minor Qt version is considered MISSING:
// the tab will show an explanation instead of crashing, and a usable build from
// another directory stays in use.

/// The marker after which the `.note.qt.metadata` note of a Qt 6 plugin has the
/// bytes "format version, major, minor" of the Qt version it was built for (the
/// `QT_PLUGIN_METADATA_SECTION` / `qt_plugin_query_metadata_v2` macro).
pub const QT_PLUGIN_NOTE_MARK: &[u8] = b"qt-project!\0";

/// The Qt version (major, minor) a plugin was built for, from the bytes of its
/// file. `None` — there is no marker (not a Qt 6 plugin) or the file is
/// truncated.
pub fn plugin_qt_version(bytes: &[u8]) -> Option<(u32, u32)> {
    let at = bytes
        .windows(QT_PLUGIN_NOTE_MARK.len())
        .position(|w| w == QT_PLUGIN_NOTE_MARK)?;
    let rest = &bytes[at + QT_PLUGIN_NOTE_MARK.len()..];
    // [metadata format version, QT_VERSION_MAJOR, QT_VERSION_MINOR, …]
    match rest {
        [_, major, minor, ..] if *major >= 5 => Some((*major as u32, *minor as u32)),
        _ => None,
    }
}

/// Whether the `Qcm.Material` build in this directory fits the running Qt: the
/// file exists, and the build's minor Qt version matches the running one (when
/// both are known).
pub fn qcm_usable<FS: ModuleFs>(fs: &mut FS, dir: &str) -> bool {
    let path = plugin_path(dir, QCM_MATERIAL_REL);
    if !fs.is_file(&path) {
        return false;
    }
    qt_versions_compatible(fs.plugin_qt_version(&path), fs.running_qt_version())
}

/// Whether the major and minor Qt versions match. Unknown on either side —
/// "they match": the check must not disable the tab where it knows nothing.
pub fn qt_versions_compatible(built: Option<(u32, u32)>, running: Option<(u32, u32)>) -> bool {
    match (built, running) {
        (Some(b), Some(r)) => b == r,
        _ => true,
    }
}

/// A `Qcm.Material` build that was found but built for another minor Qt version:
/// `(plugin path, build version, running Qt version)`. The first one in order of
/// preference is taken — that is the one the placeholder will name.
pub fn incompatible_qcm<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Option<(String, (u32, u32), (u32, u32))> {
    let running = fs.running_qt_version()?;
    for d in dirs {
        let path = plugin_path(d, QCM_MATERIAL_REL);
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(built) = fs.plugin_qt_version(&path) {
            if built != running {
                return Some((path, built, running));
            }
        }
    }
    None
}

/// Module directories without unusable builds: without those holding a
/// `Qcm.Material` for another Qt version, and without those whose
/// `waywallen.control` was built from another protocol (see
/// [`stale_control_dirs`]).
///
/// Such a directory is thrown out ENTIRELY, not just its unusable module: hand
/// it to the engine for the sake of a neighboring module — and `import` will
/// find the unusable build in it before the usable one.
pub fn usable_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    let by_qt = qt_usable_dirs(fs, dirs);
    let stale: Vec<String> = stale_control_among(fs, &by_qt)
        .into_iter()
        .map(|(dir, _)| dir)
        .collect();
    if stale.is_empty() {
        return by_qt;
    }
    let mut out: Vec<String> = by_qt.iter().filter(|d| !stale.contains(d)).cloned().collect();
    // A stale directory may carry the ONLY usable `Qcm.Material` (the
    // styling was built at home, the value types came with the package).
    // Then it is not thrown out but goes to the end of the list: the fresh
    // `waywallen.control` comes earlier and shadows the stale one, while the
    // styling stays in use.
    if !out.iter().any(|d| fs.is_file(&plugin_path(d, QCM_MATERIAL_REL))) {
        out.extend(
            stale
                .iter()
                .filter(|d| fs.is_file(&plugin_path(d, QCM_MATERIAL_REL)))
                .cloned(),
        );
    }
    out
}

/// Module directories without those holding a `Qcm.Material` for another Qt version.
fn qt_usable_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    dirs.iter()
        .filter(|d| {
            let path = plugin_path(d, QCM_MATERIAL_REL);
            !fs.is_file(&path) || qcm_usable(fs, d)
        })
        .cloned()
        .collect()
}

// ============================================================================
// The protocol `waywallen.control` was built from
// ============================================================================
//
// Warning — why (caught on 10.10.2026). `waywallen.control` is the Qt-Protobuf
// value types and enums generated from `control.proto` and `filter.proto`. The
// vendored pages are written for the protocol of the CURRENT release, while the
// module build in the home folder (`tools/build_qml_control.sh`) stands first
// in the directory list and does not update itself. After a new protocol was
// adopted, it shadowed the fresh packaged one: it had no `Flip` enum, and
// "Monitors" spewed `TypeError: Cannot read property 'FLIP_NONE' of undefined`.
//
// The sign of freshness is a mark next to the module: the SHA-256 of exactly
// those two `.proto` files the module was generated from. The module build
// script writes it (it also builds the package's module), and Rexon compares it
// with its own — the sum of the same files embedded into the core at build
// time. Why this way:
//   * the sum knows no name from the protocol — it will trigger on any
//     future update, not only on `Flip`;
//   * the check is reading a 65-byte file: neither parsing the `.so` nor
//     launching processes;
//   * the release tag (`UPSTREAM_TAG`) is no good: our additions lie on top of
//     the vendored protocol, and the protocol changes under the same tag;
//   * the file time is no good: the package and the home build are made on
//     different machines, and `cp -a` and unpacking carry the time over.
// The price — editing a single comment in a `.proto` also declares previous
// builds stale. This is harmless: a stale build is dropped only when there is a
// fresh one next to it.

/// The `waywallen.control` build mark inside the module directory: one line, the
/// SHA-256 (hex) of the concatenation `control.proto` + `filter.proto` the module
/// was generated from. Written by `tools/build_qml_control.sh`.
pub const CONTROL_STAMP_REL: &str = "waywallen/control/proto.sha256";

const CONTROL_PROTO: &[u8] =
    include_bytes!("../../../data/qt_wallpaper_engine/proto/control.proto");
const FILTER_PROTO: &[u8] =
    include_bytes!("../../../data/qt_wallpaper_engine/proto/filter.proto");

/// The mark of the protocol this Rexon was built with — the same as
/// `cat control.proto filter.proto | sha256sum` in the module build script.
pub fn control_proto_stamp() -> &'static str {
    use sha2::{Digest, Sha256};
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| {
        let mut hasher = Sha256::new();
        hasher.update(CONTROL_PROTO);
        hasher.update(FILTER_PROTO);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}

/// How the `waywallen.control` build in a directory relates to the current protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlBuild {
    /// There is no module in the directory.
    Missing,
    /// The mark matched this Rexon's protocol.
    Fresh,
    /// There is a mark, but from another protocol.
    Stale,
    /// There is no mark: the build is older than the mark itself (before
    /// 10.10.2026) or was made by something other than `tools/build_qml_control.sh`.
    Unmarked,
}

/// State of the `waywallen.control` build in a module directory.
pub fn control_build<FS: ModuleFs>(fs: &mut FS, dir: &str) -> ControlBuild {
    if !fs.is_file(&plugin_path(dir, WAYWALLEN_CONTROL_REL)) {
        return ControlBuild::Missing;
    }
    match fs.read_text(&plugin_path(dir, CONTROL_STAMP_REL)) {
        // The first word is taken: the raw output of `sha256sum` ("sum  -") is
        // also read, the case of hex does not matter.
        Some(text) => match text.split_whitespace().next() {
            Some(stamp) if stamp.eq_ignore_ascii_case(control_proto_stamp()) => {
                ControlBuild::Fresh
            }
            Some(_) => ControlBuild::Stale,
            None => ControlBuild::Unmarked,
        },
        None => ControlBuild::Unmarked,
    }
}

/// Directories whose `waywallen.control` would shadow the fresh one, and the
/// reason ([`ControlBuild::Stale`] or [`ControlBuild::Unmarked`]) — for the log
/// line. Computed over the same list the engine sees: directories with a
/// `Qcm.Material` for another Qt version are already dropped and do not count as
/// fresh.
///
/// ⭐ A build is considered stale ONLY when the list has a fresh one. If there is
/// no fresh one, nothing is dropped: an old module is better than none (most
/// pages work on it). Hence the rule for a directory without a mark too: alone
/// it stays in use, next to a fresh one it gives way.
pub fn stale_control_dirs<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Vec<(String, ControlBuild)> {
    let by_qt = qt_usable_dirs(fs, dirs);
    stale_control_among(fs, &by_qt)
}

fn stale_control_among<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Vec<(String, ControlBuild)> {
    let states: Vec<(String, ControlBuild)> = dirs
        .iter()
        .map(|d| (d.clone(), control_build(fs, d)))
        .collect();
    if !states.iter().any(|(_, s)| *s == ControlBuild::Fresh) {
        return Vec::new();
    }
    states
        .into_iter()
        .filter(|(_, s)| matches!(s, ControlBuild::Stale | ControlBuild::Unmarked))
        .collect()
}

/// The directory with a fresh `waywallen.control`, the first in order of
/// preference — the log line will name it next to the dropped one.
pub fn fresh_control_dir<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Option<String> {
    qt_usable_dirs(fs, dirs)
        .into_iter()
        .find(|d| control_build(fs, d) == ControlBuild::Fresh)
}

// ============================================================================
// QExtra: the author's real plugin or our QML substitute
// ============================================================================
//
// The vendored markup calls the `QExtra` module (`QE.AnimatedImage`,
// `QE.ImageCache`). Two builds provide it:
//   * the real plugin by the waywallen author (C++,
//     `tools/build_qml_qextra.sh`) — it lies in the module directory,
//     `QExtra/libqextraplugin.so`;
//   * our QML substitute in the vendored markup directory (`qml_ui/QExtra/`) —
//     a fallback that always works but is poorer (no shared clock and no frame
//     memory ceiling).
// The vendored markup directory stands first in the import paths, so on its
// own the substitute wins. The real plugin is taken when the executor puts its
// directory AHEAD of the vendored one — and only if everything holds:
//   1. the plugin is in a usable module directory;
//   2. next to it lies the mark of the Qt version it was built with, and its
//      major and minor match the process's Qt. The plugin calls private Qt API
//      (`QRhi`), which is not compatible between minor releases — the same class
//      as with `Qcm.Material` above. Unlike it, here an unknown version is a
//      refusal: there is somewhere to fall back to;
//   3. the scene graph renders through RHI. On a software graph (`offscreen`,
//      `QT_QUICK_BACKEND=software`) `QE.AnimatedImage` goes into the error
//      "Image requires an RHI scene graph backend" and writes it to the log on
//      every frame.

/// The real `QExtra` plugin inside the module directory; installed by
/// `tools/build_qml_qextra.sh`.
pub const QEXTRA_PLUGIN_REL: &str = "QExtra/libqextraplugin.so";

/// The mark next to the `QExtra` plugin: one line, the Qt version it was built
/// with (`6.12.0`). Written by `tools/build_qml_qextra.sh`.
pub const QEXTRA_QT_STAMP_REL: &str = "QExtra/QT_VERSION";

/// Qt platforms that have no RHI scene graph: Qt Quick itself falls back to
/// software rendering on them.
pub const SOFTWARE_ONLY_PLATFORMS: [&str; 4] = ["offscreen", "minimal", "linuxfb", "vnc"];

/// `QT_QUICK_BACKEND` values with which the scene graph is not on RHI.
pub const NON_RHI_QUICK_BACKENDS: [&str; 3] = ["software", "softwarecontext", "openvg"];

/// Whether the Qt Quick scene graph renders through RHI — by what is known
/// before the first window is created: the requested Qt platform
/// ([`requested_qpa_platform`]) and `QT_QUICK_BACKEND`.
///
/// `qpa_platform` — the raw value as it is set by the variable or the option: a
/// list separated by `;` (Qt takes the first platform that comes up), each with
/// its own parameters after `:`. We judge by the first entry. Unset — the
/// session's platform (Wayland or X11), both have RHI.
pub fn scene_graph_uses_rhi(qpa_platform: Option<&str>, quick_backend: Option<&str>) -> bool {
    let backend = quick_backend.unwrap_or("").trim().to_ascii_lowercase();
    if NON_RHI_QUICK_BACKENDS.contains(&backend.as_str()) {
        return false;
    }
    let platform = qpa_platform
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    !SOFTWARE_ONLY_PLATFORMS.contains(&platform.as_str())
}

/// Which Qt platform was requested: the value for the first argument of
/// [`scene_graph_uses_rhi`]. `args` — the process command line including the
/// zeroth argument, `env_platform` — the raw `QT_QPA_PLATFORM`.
///
/// Qt reads the variable and then the command line, and the `-platform NAME`
/// option (also `--platform NAME`) overrides the variable; of several options
/// the last one is in effect, an option without a value has no effect. Without
/// this, launching `rexon-main-window -platform offscreen` would be considered a
/// launch on the session platform and would get the real `QExtra` on a software
/// graph.
pub fn requested_qpa_platform<S: AsRef<str>>(
    args: &[S],
    env_platform: Option<&str>,
) -> Option<String> {
    let mut platform = env_platform.map(str::to_owned);
    let mut rest = args.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = rest.next() {
        if arg == "-platform" || arg == "--platform" {
            if let Some(value) = rest.next() {
                platform = Some(value.to_owned());
            }
        }
    }
    platform
}

/// The Qt version (major, minor) from the [`QEXTRA_QT_STAMP_REL`] mark: the first
/// word of the form `6.12` or `6.12.0`.
pub fn parse_qt_stamp(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Why the substitute was taken instead of the real `QExtra`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QExtraFallback {
    /// The scene graph is not on RHI — the real one does not render there.
    NoRhi,
    /// The plugin is in none of the usable module directories.
    NoPlugin,
    /// The running Qt version is unknown — there is nothing to compare the mark with.
    RunningQtUnknown { path: String },
    /// The plugin is there, but there is no Qt version mark next to it, or it cannot
    /// be read.
    NoStamp { path: String },
    /// The plugin was built for another minor Qt version.
    WrongQt {
        path: String,
        built: (u32, u32),
        running: (u32, u32),
    },
}

/// Which `QExtra` module the QML engine gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QExtraChoice {
    /// The real plugin: `dir` — the module directory the executor puts AHEAD of the
    /// vendored markup directory in the import paths.
    Real { dir: String, qt: (u32, u32) },
    /// The QML substitute from the vendored markup directory — no paths need changing.
    Shim(QExtraFallback),
}

/// Choose the `QExtra` module. `dirs` — the full list of module directories
/// ([`module_dirs`]), `rhi` — the answer of [`scene_graph_uses_rhi`].
///
/// It is searched for in the usable directories ([`usable_dirs`]) in their
/// order; the first plugin that passes the check is taken. An unusable one does
/// not block a usable one in the next directory, and the reason for the refusal
/// is named by the first one encountered.
///
/// Warning: a directory with a stale `waywallen.control` ([`stale_control_dirs`])
/// does not count, even when it is kept at the tail of the list for the sake of
/// `Qcm.Material`: the chosen directory goes first in the import paths, and the
/// stale module in it would shadow the fresh one again.
pub fn choose_qextra<FS: ModuleFs>(fs: &mut FS, dirs: &[String], rhi: bool) -> QExtraChoice {
    if !rhi {
        return QExtraChoice::Shim(QExtraFallback::NoRhi);
    }
    let stale: Vec<String> = stale_control_dirs(fs, dirs)
        .into_iter()
        .map(|(dir, _)| dir)
        .collect();
    let running = fs.running_qt_version();
    let mut first_reject: Option<QExtraFallback> = None;
    for dir in usable_dirs(fs, dirs) {
        if stale.contains(&dir) {
            continue;
        }
        let path = plugin_path(&dir, QEXTRA_PLUGIN_REL);
        if !fs.is_file(&path) {
            continue;
        }
        let Some(running) = running else {
            return QExtraChoice::Shim(QExtraFallback::RunningQtUnknown { path });
        };
        let stamp = fs
            .read_text(&plugin_path(&dir, QEXTRA_QT_STAMP_REL))
            .and_then(|text| parse_qt_stamp(&text));
        let reject = match stamp {
            None => QExtraFallback::NoStamp { path },
            Some(built) if built != running => QExtraFallback::WrongQt { path, built, running },
            // The mark matched, but the plugin itself names another Qt version
            // (the file was replaced, the mark was left) — we trust the plugin.
            Some(_) => match fs.plugin_qt_version(&path) {
                Some(built) if built != running => {
                    QExtraFallback::WrongQt { path, built, running }
                }
                _ => return QExtraChoice::Real { dir, qt: running },
            },
        };
        first_reject.get_or_insert(reject);
    }
    QExtraChoice::Shim(first_reject.unwrap_or(QExtraFallback::NoPlugin))
}

/// A log line about the choice of the `QExtra` module: what was taken and why.
pub fn qextra_choice_line(choice: &QExtraChoice) -> String {
    const SHIM: &str = "QExtra: взята QML-подмена";
    match choice {
        QExtraChoice::Real { dir, qt } => format!(
            "QExtra: взят настоящий плагин {} — собран под Qt {}.{}, как запущенный, граф сцены на RHI",
            plugin_path(dir, QEXTRA_PLUGIN_REL),
            qt.0,
            qt.1
        ),
        QExtraChoice::Shim(QExtraFallback::NoRhi) => {
            format!("{SHIM} — граф сцены не на RHI, настоящий плагин на нём не рисует")
        }
        QExtraChoice::Shim(QExtraFallback::NoPlugin) => {
            format!("{SHIM} — настоящего плагина нет в каталогах модулей")
        }
        QExtraChoice::Shim(QExtraFallback::RunningQtUnknown { path }) => {
            format!("{SHIM} — версия запущенного Qt неизвестна, плагин не сверить: {path}")
        }
        QExtraChoice::Shim(QExtraFallback::NoStamp { path }) => format!(
            "{SHIM} — рядом с плагином нет отметки версии Qt ({QEXTRA_QT_STAMP_REL}): {path}"
        ),
        QExtraChoice::Shim(QExtraFallback::WrongQt { path, built, running }) => format!(
            "{SHIM} — плагин собран под Qt {}.{}, а запущен Qt {}.{}: {path}",
            built.0, built.1, running.0, running.1
        ),
    }
}

// ============================================================================
// Directories where the built QML plugins lie
// ============================================================================

/// Template of the Debian and Ubuntu multiarch directory
/// (`/usr/lib/x86_64-linux-gnu/…`).
///
/// The caller expands it (`glob.glob`) and gives the result to [`module_dirs`]
/// already sorted: walking directories is a side effect.
pub const MULTIARCH_GLOB: &str = "/usr/lib/*/rexon/qml_modules";

/// Tail of the module directory path inside a data root.
pub const MODULES_REL: &str = "rexon/qml_modules";

/// System module directories of a fixed layout: Arch puts plugins into
/// `/usr/lib`, Fedora and openSUSE into `/usr/lib64`.
///
/// The third one is an installation into the system (`/usr/local`): its layout
/// is the same as the package's (`installer_exec::InstallLayout`). ⭐ Fix of
/// 15.09.2026: previously the manual installation directory was not searched at
/// all, and after it the Rust window did not find the wallpaper tab's plugins.
pub const SYSTEM_MODULE_DIRS: [&str; 3] = [
    "/usr/lib/rexon/qml_modules",
    "/usr/lib64/rexon/qml_modules",
    "/usr/local/lib/rexon/qml_modules",
];

/// All directories where built QML plugins may lie, in decreasing order of
/// preference and without duplicates.
///
/// The order is deliberate: the user build (the developer makes it and expects
/// to see exactly it) → data directories from `$XDG_DATA_DIRS` → fixed system
/// ones → multiarch directories. This is the DEVELOPMENT order: for a package
/// and an installation it is rearranged by [`order_module_dirs`].
///
/// `xdg_data_home` — `$XDG_DATA_HOME` (Fix 3), `xdg_data_dirs` — the raw value
/// of `$XDG_DATA_DIRS` separated by colons, `multiarch` — the already expanded
/// and sorted [`MULTIARCH_GLOB`].
pub fn module_dirs(
    xdg_data_home: Option<&str>,
    home: &str,
    xdg_data_dirs: &str,
    multiarch: &[String],
) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    // Fix 3: `$XDG_DATA_HOME` is respected, but `~/.local/share` remains a
    // candidate — `tools/build_qml_*.sh` write there unconditionally, and with
    // a non-standard XDG an already made build would otherwise be lost.
    dirs.push(format!("{}/{MODULES_REL}", data_home(xdg_data_home, home)));
    dirs.push(format!("{}/{MODULES_REL}", data_home(None, home)));
    for base in xdg_data_dirs.split(':') {
        let base = base.trim();
        if !base.is_empty() {
            dirs.push(format!("{}/{MODULES_REL}", base.trim_end_matches('/')));
        }
    }
    dirs.extend(SYSTEM_MODULE_DIRS.iter().map(|d| d.to_string()));
    // ⭐ Fix of 15.09.2026: an installation into the home folder has the same
    // layout (`installer_exec::InstallLayout`), and its module directory was
    // not searched. The place is right after the system installation, as for
    // the window binary (`installer::main_window_candidates`).
    if let Some(lib) = crate::installer::user_install_lib_dir(home) {
        dirs.push(format!("{lib}/{}", crate::installer_exec::QML_MODULES_DIR_NAME));
    }
    dirs.extend(multiarch.iter().cloned());

    let mut seen: Vec<String> = Vec::new();
    dirs.retain(|d| {
        if seen.iter().any(|s| s == d) {
            false
        } else {
            seen.push(d.clone());
            true
        }
    });
    dirs
}

// ============================================================================
// The user build versus the packaged one
// ============================================================================
//
// Warning — why (caught on 02.11.2026). [`module_dirs`] puts the user directory
// (`~/.local/share/rexon/qml_modules`, where `tools/build_qml_*.sh` write)
// FIRST — that is the developer's order. But the same order also applied to
// the distribution package: on a machine where modules had once been built by
// hand, the installed Rexon took `QExtra`, `Qcm.Material` and
// `waywallen.control` from the old manual build rather than from its package.
// While the revisions match this is unnoticeable; after a package update a
// stale copy that the package manager never updates would come into use.
//
// The rule: the user directory takes precedence only when running from the
// source tree (development). For a package and an installation their own copy
// takes precedence, and the user one is taken only when the package has no
// copy of the module. The user's files are not deleted — it is their manual
// build; the log says that it was skipped and how it differs.

/// The `Qcm.Material` revision mark next to the module; written by
/// `tools/build_qml_material.sh` (and the package build uses it too).
pub const QCM_MATERIAL_PIN_REL: &str = "Qcm/QML_MATERIAL_PIN";

/// The `QExtra` revision mark next to the module; written by `tools/build_qml_qextra.sh`.
pub const QEXTRA_PIN_REL: &str = "QExtra/QEXTRA_PIN";

/// What is checked for one module: a name for the log, the plugin file and the
/// marks next to it (label, path in the module directory).
struct ComparedModule {
    name: &'static str,
    plugin_rel: &'static str,
    stamps: &'static [(&'static str, &'static str)],
}

/// All three modules of the wallpaper tab that both `tools/build_qml_*.sh` and
/// the package put. A new module is added as a line here (rule #12).
const COMPARED_MODULES: [ComparedModule; 3] = [
    ComparedModule {
        name: "Qcm.Material",
        plugin_rel: QCM_MATERIAL_REL,
        stamps: &[("ревизия", QCM_MATERIAL_PIN_REL)],
    },
    ComparedModule {
        name: "waywallen.control",
        plugin_rel: WAYWALLEN_CONTROL_REL,
        stamps: &[("протокол", CONTROL_STAMP_REL)],
    },
    ComparedModule {
        name: "QExtra",
        plugin_rel: QEXTRA_PLUGIN_REL,
        stamps: &[("ревизия", QEXTRA_PIN_REL), ("отметка Qt", QEXTRA_QT_STAMP_REL)],
    },
];

/// User build directories — the same first candidates as in [`module_dirs`]:
/// `$XDG_DATA_HOME` and `~/.local/share`, without duplicates.
pub fn user_module_dirs(xdg_data_home: Option<&str>, home: &str) -> Vec<String> {
    let mut out = vec![format!("{}/{MODULES_REL}", data_home(xdg_data_home, home))];
    let plain = format!("{}/{MODULES_REL}", data_home(None, home));
    if !out.contains(&plain) {
        out.push(plain);
    }
    out
}

/// Whether the user build of the modules takes precedence: only for a launch
/// from the source tree, and only if the package launcher did not say "package"
/// (`installer::PACKAGED_ENV`).
pub fn user_modules_first(packaged: bool, dev_tree: bool) -> bool {
    dev_tree && !packaged
}

/// How a module of the user build differs from the packaged one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDiff {
    pub module: &'static str,
    /// What differs: "revision", "protocol", "Qt mark", "build's Qt".
    pub what: &'static str,
    /// The user's value; "none" when there is no mark.
    pub user: String,
    pub package: String,
}

/// What was done with the user module directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserCopyUse {
    /// Launch from the source tree — the directory stays first.
    DevTree,
    /// None of its modules has a packaged copy — it is taken.
    NoPackageCopy,
    /// Some modules exist only in it — it is kept BEHIND the package directories:
    /// the shared modules come from the package.
    OnlyHere(Vec<&'static str>),
    /// Everything in it is provided by the package — the directory is not used.
    Skipped,
}

/// The decision for one user directory — for the log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCopyNote {
    pub dir: String,
    /// The first non-user directory where a copy of its modules was found.
    pub package_dir: Option<String>,
    pub diffs: Vec<ModuleDiff>,
    pub used: UserCopyUse,
}

/// The order of module directories and the decisions on the user directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleOrder {
    pub dirs: Vec<String>,
    pub notes: Vec<UserCopyNote>,
}

/// A mark's value: the first word of the file.
fn stamp_value<FS: ModuleFs>(fs: &mut FS, dir: &str, rel: &str) -> Option<String> {
    fs.read_text(&plugin_path(dir, rel))
        .and_then(|t| t.split_whitespace().next().map(str::to_ascii_lowercase))
}

/// How module `m` in directory `user` differs from the same module in
/// `package`. Marks and the plugin's Qt version are compared only when the
/// package has them: if there is nothing to compare with, it is not a difference.
fn module_diffs<FS: ModuleFs>(
    fs: &mut FS,
    m: &ComparedModule,
    user: &str,
    package: &str,
) -> Vec<ModuleDiff> {
    let mut out = Vec::new();
    for (what, rel) in m.stamps {
        let Some(pkg) = stamp_value(fs, package, rel) else { continue };
        let usr = stamp_value(fs, user, rel);
        if usr.as_deref() != Some(pkg.as_str()) {
            out.push(ModuleDiff {
                module: m.name,
                what,
                user: usr.unwrap_or_else(|| "нет".to_string()),
                package: pkg,
            });
        }
    }
    let fmt = |v: (u32, u32)| format!("{}.{}", v.0, v.1);
    if let Some(pkg) = fs.plugin_qt_version(&plugin_path(package, m.plugin_rel)) {
        let usr = fs.plugin_qt_version(&plugin_path(user, m.plugin_rel));
        if usr != Some(pkg) {
            out.push(ModuleDiff {
                module: m.name,
                what: "Qt сборки",
                user: usr.map(fmt).unwrap_or_else(|| "нет".to_string()),
                package: fmt(pkg),
            });
        }
    }
    out
}

/// Arrange the module directories ([`module_dirs`]) according to where Rexon was
/// launched from. `user_dirs` — [`user_module_dirs`], `user_first` —
/// [`user_modules_first`].
///
/// * `user_first` — the order does not change (development: one's own build
///   takes precedence); differences from the packaged copy still go into the
///   notes.
/// * otherwise the package directories go first. A user directory is thrown out
///   if everything in it is in the package (matching or stale — it does not
///   matter, the package takes precedence); it is kept AT THE END of the list if
///   at least one of its modules is not in the package.
pub fn order_module_dirs<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
    user_dirs: &[String],
    user_first: bool,
) -> ModuleOrder {
    let package: Vec<String> = dirs.iter().filter(|d| !user_dirs.contains(d)).cloned().collect();
    let mut kept_users: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for user in dirs.iter().filter(|d| user_dirs.contains(d)) {
        let mut present = 0usize;
        let mut only_here = Vec::new();
        let mut diffs = Vec::new();
        let mut package_dir: Option<String> = None;
        for m in &COMPARED_MODULES {
            if !fs.is_file(&plugin_path(user, m.plugin_rel)) {
                continue;
            }
            present += 1;
            let pkg = package
                .iter()
                .find(|p| fs.is_file(&plugin_path(p, m.plugin_rel)))
                .cloned();
            match pkg {
                Some(pkg) => {
                    diffs.extend(module_diffs(fs, m, user, &pkg));
                    package_dir.get_or_insert(pkg);
                }
                None => only_here.push(m.name),
            }
        }
        if present == 0 {
            // An empty directory neither interferes nor gives anything.
            if user_first {
                kept_users.push(user.clone());
            }
            continue;
        }
        let used = if user_first {
            UserCopyUse::DevTree
        } else if only_here.len() == present {
            UserCopyUse::NoPackageCopy
        } else if !only_here.is_empty() {
            UserCopyUse::OnlyHere(only_here)
        } else {
            UserCopyUse::Skipped
        };
        if used != UserCopyUse::Skipped {
            kept_users.push(user.clone());
        }
        notes.push(UserCopyNote { dir: user.clone(), package_dir, diffs, used });
    }
    let dirs = if user_first {
        dirs.iter()
            .filter(|d| !user_dirs.contains(d) || kept_users.contains(d))
            .cloned()
            .collect()
    } else {
        package.into_iter().chain(kept_users).collect()
    };
    ModuleOrder { dirs, notes }
}

/// A log line about the user module directory and its level: `true` — a warning
/// (the copy differs from the packaged one), `false` — information. `None` —
/// nothing to say (development without differences).
pub fn user_copy_note_line(note: &UserCopyNote) -> Option<(bool, String)> {
    let diffs = note
        .diffs
        .iter()
        .map(|d| format!("{} — {}: у пользователя {}, в пакете {}", d.module, d.what, d.user, d.package))
        .collect::<Vec<_>>()
        .join("; ");
    let pkg = note.package_dir.as_deref().unwrap_or("?");
    let warn = !note.diffs.is_empty();
    let line = match &note.used {
        UserCopyUse::DevTree if warn => format!(
            "пользовательская сборка модулей {} отличается от пакетной ({pkg}): {diffs}; \
             запуск из дерева исходников — берётся пользовательская",
            note.dir
        ),
        UserCopyUse::DevTree => return None,
        UserCopyUse::NoPackageCopy => format!(
            "пакетной копии QML-модулей нет — берётся пользовательская сборка {}",
            note.dir
        ),
        UserCopyUse::OnlyHere(names) => format!(
            "пользовательская сборка модулей {} оставлена позади пакетной ({pkg}): только в ней есть {}{}",
            note.dir,
            names.join(", "),
            if warn { format!("; общие модули отличаются и берутся из пакета: {diffs}") } else { String::new() }
        ),
        UserCopyUse::Skipped if warn => format!(
            "пользовательская сборка модулей {} устарела относительно пакетной ({pkg}) и не используется: \
             {diffs}. Файлы не тронуты; если сборка не нужна, каталог можно удалить вручную",
            note.dir
        ),
        UserCopyUse::Skipped => format!(
            "пользовательская сборка модулей {} не используется: Rexon поставлен пакетом, \
             берётся пакетная копия {pkg}",
            note.dir
        ),
    };
    Some((warn, line))
}

// ============================================================================
// What exactly we look for in these directories
// ============================================================================

/// `Qcm.Material` — a compiled QML plugin (MIT, QmlMaterial by hypengw),
/// installed by `tools/build_qml_material.sh`.
pub const QCM_MATERIAL_REL: &str = "Qcm/Material/libqml_materialplugin.so";

/// `waywallen.control` — Qt-Protobuf value types that PySide6 cannot provide;
/// installed by `tools/build_qml_control.sh`.
pub const WAYWALLEN_CONTROL_REL: &str =
    "waywallen/control/libwaywallen-controlplugin.so";

/// The library of the `waywallen.control` value types themselves (the plugin
/// above is its thin QML wrapper). The types are registered when the library is
/// loaded, so a test stand without a QML engine only needs to load it.
pub const WAYWALLEN_CONTROL_TYPES_REL: &str = "waywallen/control/libwaywallen-control.so";

/// Where `tools/build_qml_control.sh` puts our Rust builds next to the C++
/// plugin: a PACKAGE has no `rust/target` tree at all.
pub const RUST_LIB_SUBDIR: &str = "waywallen/control";

/// The `rexon-qt-filter` crate (cxx-qt): the model of the wallpaper filter editor.
pub const FILTER_LIB_NAME: &str = "librexon_qt_filter.so";

/// Build profiles in the developer tree, in decreasing order of freshness.
pub const TARGET_PROFILES: [&str; 2] = ["release", "debug"];

/// Full path of a plugin in the module directory.
pub fn plugin_path(modules_dir: &str, rel: &str) -> String {
    format!("{}/{rel}", modules_dir.trim_end_matches('/'))
}

/// The first directory in the list where the needed plugin lies.
pub fn find_plugin<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
    rel: &str,
) -> Option<String> {
    dirs.iter()
        .map(|d| plugin_path(d, rel))
        .find(|p| fs.is_file(p))
}

/// The directory given to the QML engine as the first `addImportPath` — the one
/// where `Qcm.Material` was found; if there is none, the first candidate (so that
/// the error message has a meaningful path).
///
/// Port of `_resolve_qcm_dir` together with its fallback branch.
pub fn resolve_import_dir<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> String {
    for d in dirs {
        if qcm_usable(fs, d) {
            return d.clone();
        }
    }
    dirs.first().cloned().unwrap_or_default()
}

/// All module directories in which some of our plugins actually lie — they
/// should be given to the QML engine in `addImportPath`.
///
/// Fix 2 from the other side: while there was ONE import path, a split
/// installation (`Qcm.Material` built by the user, `waywallen.control` came with
/// the package) did not work — half of the types were not imported. The list
/// keeps the previous order of preference, so for an ordinary installation into
/// one directory the answer is the same as before.
pub fn import_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    usable_dirs(fs, dirs)
        .iter()
        .filter(|d| {
            fs.is_file(&plugin_path(d, QCM_MATERIAL_REL))
                || fs.is_file(&plugin_path(d, WAYWALLEN_CONTROL_REL))
        })
        .cloned()
        .collect()
}

/// Whether BOTH mandatory QML plugins are present (otherwise the tab's QML
/// interface is unavailable) — port of `qcm_material_available`.
///
/// Exactly two are needed: `Qcm.Material` provides the styling,
/// `waywallen.control` the Qt-Protobuf value types without which the
/// "Wallpapers" page cannot be built.
///
/// Directories with a `Qcm.Material` for another Qt version and with a
/// `waywallen.control` from another protocol (with a live fresh one) do not
/// count — see [`usable_dirs`].
pub fn qml_plugins_ready<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> bool {
    let dirs = usable_dirs(fs, dirs);
    find_plugin(fs, &dirs, QCM_MATERIAL_REL).is_some()
        && find_plugin(fs, &dirs, WAYWALLEN_CONTROL_REL).is_some()
}

/// Candidates for a Rust build in decreasing order of freshness: `release` →
/// `debug` in the developer tree → the QML module directories.
///
/// Fixes 1 and 2: the module directories go ALL and in the same order as in
/// [`module_dirs`], not a single one chosen by `Qcm.Material`.
pub fn rust_lib_candidates(
    repo_root: &str,
    dirs: &[String],
    lib_name: &str,
) -> Vec<String> {
    let target = format!("{}/rust/target", repo_root.trim_end_matches('/'));
    let mut out: Vec<String> = TARGET_PROFILES
        .iter()
        .map(|p| format!("{target}/{p}/{lib_name}"))
        .collect();
    out.extend(
        dirs.iter()
            .map(|d| plugin_path(d, &format!("{RUST_LIB_SUBDIR}/{lib_name}"))),
    );
    out
}

/// Candidates for the build of the `rexon-qt-filter` crate — port of
/// `bridge._rust_filter_model_candidates`.
pub fn filter_lib_candidates(repo_root: &str, dirs: &[String]) -> Vec<String> {
    rust_lib_candidates(repo_root, dirs, FILTER_LIB_NAME)
}

/// The first existing candidate or `None`.
pub fn pick_existing<FS: ModuleFs>(fs: &mut FS, candidates: &[String]) -> Option<String> {
    candidates.iter().find(|p| fs.is_file(p)).cloned()
}

// ============================================================================
// Paths of the vendored QML and the thumbnail disk cache
// ============================================================================

/// The tab's root QML host: a `Loader` holding the page being shown, so that
/// there remains ONE QML engine for all pages.
pub const PAGE_HOST_QML: &str = "PageHost.qml";

/// The warm-up scene: the registration of the `waywallen.control` Qt-Protobuf
/// module must happen on the MAIN thread before the first `QQuickWidget`.
pub const PREWARM_QML: &str = "_prewarm_control.qml";

/// The file with the vendored upstream tag inside the tab's QML directory.
pub const UPSTREAM_TAG_REL: &str = "waywallen/ui/UPSTREAM_TAG";

/// The directory of vendored QML pages inside the tab's directory.
pub const VENDOR_PAGE_DIR: &str = "waywallen/ui/qml/page";

/// The answer of [`upstream_tag`] when the file is missing or could not be read.
pub const UPSTREAM_TAG_UNKNOWN: &str = "?";

/// Path of a file inside the tab's QML directory (`qml_ui/`).
pub fn qml_path(qml_dir: &str, rel: &str) -> String {
    format!("{}/{rel}", qml_dir.trim_end_matches('/'))
}

/// Path of a vendored page by its name (`"DiscoverPage.qml"`); an empty name —
/// an empty answer, the host shows an empty `Loader`.
pub fn vendor_page_path(qml_dir: &str, page_qml: &str) -> String {
    if page_qml.is_empty() {
        return String::new();
    }
    qml_path(qml_dir, &format!("{VENDOR_PAGE_DIR}/{page_qml}"))
}

/// The vendored upstream tag from the file contents that were read.
pub fn upstream_tag(raw: Option<&str>) -> String {
    match raw {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        // An empty file used to be returned as an empty string and went right
        // into the window title ("waywallen "). The same answer as for a missing
        // file is more honest: we do not know the tag (rule #13).
        _ => UPSTREAM_TAG_UNKNOWN.to_string(),
    }
}

/// Version of the vendored interface from the contents of `UPSTREAM_TAG`:
/// `v0.5.0 (564eb02)` → `0.5.0`.
///
/// This is the same number as the upstream's `Qt.application.version` (the
/// version of the `waywallen-ui` package), and of the same form as the daemon
/// version over D-Bus — the "About" page compares them and shows a second line
/// "Daemon version" only when they differ (our patch 0002). No tag — the same
/// answer as [`upstream_tag`]: an honest "?", not an empty string.
pub fn ui_version(raw: Option<&str>) -> String {
    let tag = upstream_tag(raw);
    let first = tag.split_whitespace().next().unwrap_or(UPSTREAM_TAG_UNKNOWN);
    let version = first.strip_prefix('v').unwrap_or(first);
    if version.is_empty() {
        UPSTREAM_TAG_UNKNOWN.to_string()
    } else {
        version.to_string()
    }
}

/// Tail of the disk cache directory of QML network pictures (Discover thumbnails).
pub const NET_CACHE_REL: &str = "rexon/qml_net";

/// Limit of the thumbnail disk cache, bytes.
pub const NET_CACHE_MAX_BYTES: i64 = 200 * 1024 * 1024;

/// The thumbnail disk cache directory. `$XDG_CACHE_HOME` is respected — this is
/// the same class as Fix 3 (the original had a literal `~/.cache`).
pub fn net_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(x) if !x.is_empty() => x.trim_end_matches('/').to_string(),
        _ => format!("{}/.cache", home.trim_end_matches('/')),
    };
    format!("{base}/{NET_CACHE_REL}")
}
