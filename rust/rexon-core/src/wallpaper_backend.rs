//! Who draws live wallpapers in the current session — one decision for all of Rexon.
//!
//! Before this module the knowledge was smeared around: `_is_kde()` in
//! `waywallen_kde_plugin`, `is_wayland_session()` in `wallpaper_mmap`,
//! `if self.de == "kde"` branches in the old wallpaper engine. Because of
//! this, paths appeared that fought over the same screen. Here is one
//! function, [`pick`], used by everyone (rule #12).
//!
//! The decision mirrors the choice of the OUTPUT BACKEND in the waywallen
//! daemon itself (since 0.5.0: definitions — `src/plugin/display_registry.rs`,
//! `builtin_display_defs`; choice — `src/wallframe/display/spawner.rs`,
//! `detect_de` + `pick_backend`; checked 24.10.2026) — it has to be known
//! because the launch mode decides whether Rexon has to do anything:
//!
//! * `kde-plasma` → `SpawnMode::External`, priority 100, a hard rule "KDE
//!   always goes here and NEVER falls back". External = the daemon does not
//!   launch the plugin, someone outside installs and selects it. Outside is
//!   us: Rexon installs the `org.waywallen.kde` kpackage via `kpackagetool6`
//!   and selects it on the desktop containments.
//! * `layer-shell` → `SpawnMode::Daemon`, priority 50, desktop list
//!   `hyprland, sway, niri, river, cosmic, wayfire`, requirements
//!   `wlr-layer-shell` + `linux-dmabuf-v4`. Daemon = the daemon ITSELF finds
//!   the `waywallen-layer-shell` binary next to itself and launches it.
//!   **Rexon does nothing here** — it is enough that the daemon is up. We do
//!   not need a path of our own for these sessions.
//!
//! Hence the main conclusion this module was written for: our own subprocess
//! (the `rexon-wallpaper-host` host on LayerShellQt) on wlr compositors is
//! SUPERFLUOUS, it duplicates the stock `waywallen-layer-shell` and fights it
//! for the surface. Its real niche is X11. The daemon itself does not depend
//! on X11 (it draws into dma-buf and hands frames out to output clients); on
//! X11 only the output CLIENT is missing: `waywallen-layer-shell` is Wayland.
//! The exception is Plasma on X11: the `org.waywallen.kde` plugin works there
//! too (plasmashell needs `QT_XCB_GL_INTEGRATION=xcb_egl`, the
//! waywallen-display README), and the daemon's hard KDE rule does not look at
//! the session type. For now [`pick`] sends any X11 to our process, KDE too:
//! moving Plasma-X11 to the plugin is a separate decision (a plasmashell
//! setting), not taken here.
//!
//! ⚠️ `river` is in the daemon's list but missing from the support table of
//! their README — we take it from the code as is, the code is more honest
//! than the table.
//!
//! ⚠️ Wayfire does not set `XDG_CURRENT_DESKTOP` itself, so the daemon also
//! recognizes it by the socket: if the `XDG_CURRENT_DESKTOP` tokens contain
//! nothing but `wayland` (or the variable is empty) and `WAYFIRE_SOCKET` is
//! non-empty, `wayfire` is appended to the tokens (`desktop_tokens` in
//! spawner.rs). The same rule is in [`session_desktops`].
//!
//! One thing we deliberately do not mirror: the 0.5.0 daemon also has a
//! `gnome-shell` definition (`SpawnMode::External`) — it is installed by the
//! shell extension from waywallen itself. Rexon has its own extension on
//! GNOME/Cinnamon (`ShellExtension` below), and the daemon's choice does not
//! affect this decision.

/// Who exactly shows the live wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperBackend {
    /// KDE Plasma: the `org.waywallen.kde` Plasma plugin inside the containment.
    /// Desktop icons and clicks stay in place.
    /// **Rexon must** install the plugin and select it.
    KdePlugin,
    /// wlr compositors: the daemon starts `waywallen-layer-shell` itself.
    /// **Rexon does nothing** beyond starting the daemon.
    DaemonLayerShell,
    /// GNOME/Cinnamon on Wayland: there is no layer-shell (Mutter and its fork
    /// Muffin do not implement it), so frames go to the shell extension through
    /// an mmap file.
    ShellExtension,
    /// X11: waywallen has no output client of its own here. Video, web and Flash
    /// are drawn by our host; Wallpaper Engine scenes are drawn by the daemon,
    /// and the same host in output client mode puts them on the desktop
    /// ([`DisplaySetup::RexonHost`]). Windows are sent to the background via
    /// `_NET_WM_WINDOW_TYPE_DESKTOP`.
    X11Subprocess,
    /// A Wayland session on a compositor the daemon does not know.
    /// The daemon goes headless, there will be no wallpaper.
    Unsupported,
}

/// Desktops on which the daemon starts layer-shell itself
/// (`builtin_display_defs`, the `de` field of the `layer-shell` definition).
pub const DAEMON_LAYER_SHELL_DESKTOPS: &[&str] =
    &["hyprland", "sway", "niri", "river", "cosmic", "wayfire"];

/// Shells to which frames are handed by an extension through mmap.
///
/// This is Rexon's OWN path (our `rexon-wallpaper-host` + the
/// `rexon-wallpaper@rexon.app` extension for GNOME and Cinnamon; Unity is the
/// same Mutter family), the waywallen daemon does not know it and takes no
/// part in the choice. The similarity to the daemon's `gnome-shell`
/// definition is only in the name.
pub const SHELL_EXTENSION_DESKTOPS: &[&str] = &["gnome", "cinnamon", "unity"];

impl WallpaperBackend {
    /// Whether an action is required from Rexon for the wallpaper to go.
    ///
    /// Only KDE: there the daemon declares `spawn=External` and expects someone
    /// outside to install and select the plugin.
    pub fn needs_rexon_setup(self) -> bool {
        self == WallpaperBackend::KdePlugin
    }

    /// Whether the waywallen engine draws the wallpaper itself (and not our code).
    pub fn is_waywallen(self) -> bool {
        matches!(
            self,
            WallpaperBackend::KdePlugin | WallpaperBackend::DaemonLayerShell
        )
    }

    /// An explanation for the user — human, without jargon (rule #13: we do not
    /// show binary and protocol names to the customer).
    pub fn human_note(self) -> &'static str {
        match self {
            WallpaperBackend::KdePlugin => {
                "Живые обои встраиваются в рабочий стол Plasma — значки и \
                 щелчки по столу работают как обычно."
            }
            WallpaperBackend::DaemonLayerShell => {
                "Живые обои показывает движок обоев — отдельной настройки \
                 не требуется."
            }
            WallpaperBackend::ShellExtension => {
                "В этой оболочке обои показывает расширение Rexon. Если \
                 обои не появились, включите расширение и перезайдите в \
                 систему."
            }
            WallpaperBackend::X11Subprocess => {
                "Сеанс X11: живые обои показывает собственный проигрыватель \
                 Rexon. Часть возможностей движка обоев здесь недоступна."
            }
            WallpaperBackend::Unsupported => {
                "Оконный менеджер этого сеанса не поддерживает живые обои. \
                 Статичную картинку поставить можно."
            }
        }
    }
}

/// The Wayland session sign: `XDG_SESSION_TYPE=wayland` OR a non-empty
/// `WAYLAND_DISPLAY` (the second helps where session_type is not set —
/// nested sessions, launching from a TTY).
pub fn is_wayland_session(session_type: &str, wayland_display: &str) -> bool {
    session_type.trim().eq_ignore_ascii_case("wayland") || !wayland_display.trim().is_empty()
}

/// Whether the token is among the `XDG_CURRENT_DESKTOP` values (it is
/// colon-separated and does not guarantee case: «KDE», «Hyprland»,
/// «pop:GNOME»).
fn has_desktop(xdg_current_desktop: &str, wanted: &[&str]) -> bool {
    xdg_current_desktop.split(':').any(|token| {
        let t = token.trim();
        wanted.iter().any(|w| t.eq_ignore_ascii_case(w))
    })
}

/// The session's desktops as the daemon sees them: `XDG_CURRENT_DESKTOP`
/// and, if it says nothing (empty or only `wayland`) while `WAYFIRE_SOCKET`
/// is non-empty, `wayfire`. Wayfire does not set this variable itself.
pub fn session_desktops(xdg_current_desktop: &str, wayfire_socket: &str) -> String {
    let silent = xdg_current_desktop
        .split(':')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .all(|t| t.eq_ignore_ascii_case("wayland"));
    if silent && !wayfire_socket.is_empty() {
        if xdg_current_desktop.trim().is_empty() {
            "wayfire".to_string()
        } else {
            format!("{xdg_current_desktop}:wayfire")
        }
    } else {
        xdg_current_desktop.to_string()
    }
}

/// Pick the wallpaper backend for the session.
///
/// The order of checks repeats the daemon: first X11 is cut off (waywallen
/// cannot do it), then the hard KDE rule, then the list of wlr compositors,
/// then the shells with an extension. `wayfire_socket` — the value of
/// `WAYFIRE_SOCKET` (empty if not set), see [`session_desktops`].
pub fn pick(
    xdg_current_desktop: &str,
    session_type: &str,
    wayland_display: &str,
    wayfire_socket: &str,
) -> WallpaperBackend {
    if !is_wayland_session(session_type, wayland_display) {
        return WallpaperBackend::X11Subprocess;
    }
    let desktops = session_desktops(xdg_current_desktop, wayfire_socket);
    let xdg_current_desktop = desktops.as_str();
    // The daemon's hard rule: KDE always goes its own way and never falls back
    // to layer-shell (otherwise the window would cover the containment and the
    // desktop icons would disappear). The sign is exactly the daemon's
    // (`DeCaps::is_kde`): the `kde` token and only it. The daemon does not
    // recognize the former second token `plasma`: in such a session it would
    // not pick the plugin, and our plugin install would do nothing. Plasma
    // itself sets `XDG_CURRENT_DESKTOP=KDE`.
    if has_desktop(xdg_current_desktop, &["kde"]) {
        return WallpaperBackend::KdePlugin;
    }
    if has_desktop(xdg_current_desktop, DAEMON_LAYER_SHELL_DESKTOPS) {
        return WallpaperBackend::DaemonLayerShell;
    }
    if has_desktop(xdg_current_desktop, SHELL_EXTENSION_DESKTOPS) {
        return WallpaperBackend::ShellExtension;
    }
    WallpaperBackend::Unsupported
}

// ── a display for the waywallen daemon when it has none yet ─────────────
//
// ⛔ Defect of 30.10.2026 (MATE on X11, r1898): on KDE Rexon itself breaks
// the circle "the „Apply“ button waits for a display — the display appears
// after applying" (it assigns the Plasma plugin,
// `waywallen_kde_plugin::apply_gate`), while on X11 outside KDE the daemon
// has no output client at all: the daemon writes «no display backend
// matched xdg_desktop=["mate"]», zero displays, the button is grayed out,
// the «Monitors» page is silent. Wallpaper Engine scenes are drawn by the
// daemon, not by our host, so "X11 — Rexon's own player" did not apply to them.
//
// The way out without changing the daemon: Rexon's wallpaper host in
// `--mode waywallen` becomes the output client. It loads the waywallen
// output element from the same `org.waywallen.kde` package Rexon already
// ships for Plasma (the waywallen-display library, MIT, Qt6 only; on X11 —
// via EGL), and puts one desktop window per monitor. Verified on Xvfb
// 30.10.2026: the display registers, the scene runs at ~30 fps.
//
// Who starts it:
// * the daemon — on every start of its own, from the output definition in
//   `$XDG_DATA_HOME/waywallen/displays/` ([`rexon_display_manifest`]); the
//   daemon also restarts a crashed host;
// * Rexon — right on «Apply» if there are no displays: the daemon chooses
//   the output only at start, and restarting the daemon would break the
//   apply itself.
// A second host instance waits for the lock ([`rexon_host_lock_path`]) and
// picks up the output if the first one dies — there are never two windows
// per monitor.

/// How the daemon gets a display when it has none yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySetup {
    /// Rexon starts nothing here: the display is given by the daemon itself
    /// (layer-shell) or there is no output on this desktop.
    None,
    /// KDE: Rexon assigns the Plasma plugin as the desktop wallpaper.
    KdePlugin,
    /// X11: the output client is Rexon's wallpaper host (`--mode waywallen`).
    RexonHost,
    /// GNOME: the output is the shell extension from waywallen
    /// ([`WAYWALLEN_GNOME_EXT_ID`]); Rexon disables it for the sake of its own
    /// wallpaper and enables it on «Apply» ([`set_waywallen_gnome_display`]).
    /// It is never a pure backend answer — only [`display_setup_for`].
    GnomeExtension,
}

impl WallpaperBackend {
    /// What Rexon does so that the daemon gets a display.
    pub fn display_setup(self) -> DisplaySetup {
        match self {
            WallpaperBackend::KdePlugin => DisplaySetup::KdePlugin,
            WallpaperBackend::X11Subprocess => DisplaySetup::RexonHost,
            WallpaperBackend::DaemonLayerShell
            | WallpaperBackend::ShellExtension
            | WallpaperBackend::Unsupported => DisplaySetup::None,
        }
    }
}

impl DisplaySetup {
    /// Whether Rexon brings up the display itself (then «Apply» is available even
    /// without displays, and the apply frame waits for a display).
    pub fn rexon_brings_display(self) -> bool {
        self != DisplaySetup::None
    }
}

// ── GNOME: the waywallen output extension and non-waywallen wallpapers ──
//
// ⛔ Defect of 09.11.2026 (GNOME 51 Wayland, waywallen extension 0.4.5): with
// the `org.waywallen.gnome@waywallen.io` extension enabled and no live
// wallpaper the desktop is BLACK — that is by their design (`_showFallback`,
// «the desktop keeps the disconnected fallback black»). When setting a
// picture, Rexon stopped waywallen's renderers and wrote `picture-uri`, but
// the picture was covered by the extension's black placeholder. The same
// with video, GIF and Flash via our `rexon-wallpaper@rexon.app` extension:
// both hook into the same `BackgroundManager`.
//
// One decision for all formats: non-waywallen wallpapers (their list is
// `wallpaper_exec::yields_we_wallpapers`) disable the waywallen extension,
// waywallen wallpapers (the WE tab, «Apply» and the playlist) enable it back
// — through the same frame hold-back path as KDE and X11
// ([`DisplaySetup::GnomeExtension`]): without the extension the daemon has
// zero displays, and «Apply» would otherwise refuse (`NoDisplayRegistered`).
// On/off is done by a call to the shell itself over the bus
// (`EnableExtension`), the fallback path is the `enabled-extensions` list
// via the shared `de_customization` scripts without losing foreign uuids.
//
// Other desktops: KDE — `plasma-apply-wallpaperimage` itself switches the
// desktop plugin to `org.kde.image`, no conflict; X11 — the waywallen output
// there is our host (`RexonHost`), its placeholder was not dealt with here.

/// The GNOME shell extension with which waywallen shows wallpapers.
pub const WAYWALLEN_GNOME_EXT_ID: &str = "org.waywallen.gnome@waywallen.io";

/// A GNOME Shell session (and not a desktop that merely appends `GNOME` to
/// its name: `Budgie:GNOME`, `X-Cinnamon`, `Unity`, `Pantheon`).
pub fn is_gnome_shell_session(xdg_current_desktop: &str) -> bool {
    has_desktop(xdg_current_desktop, &["gnome"])
        && !has_desktop(
            xdg_current_desktop,
            &["budgie", "budgie-desktop", "x-cinnamon", "cinnamon", "unity", "pantheon"],
        )
}

/// Directories where the `uuid` extension may lie: the user one
/// (`$XDG_DATA_HOME`, otherwise `~/.local/share`) and the system ones from
/// `$XDG_DATA_DIRS` (empty — `/usr/local/share:/usr/share`) — the shell's own
/// lookup order.
pub fn gnome_extension_dirs(uuid: &str, xdg_data_home: &str, home: &str, xdg_data_dirs: &str) -> Vec<String> {
    let user = if xdg_data_home.trim().is_empty() {
        format!("{}/.local/share", home.trim_end_matches('/'))
    } else {
        xdg_data_home.trim_end_matches('/').to_string()
    };
    let system = if xdg_data_dirs.trim().is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
    std::iter::once(user.as_str())
        .chain(system.split(':').map(|d| d.trim_end_matches('/')).filter(|d| !d.is_empty()))
        .map(|base| format!("{base}/gnome-shell/extensions/{uuid}"))
        .collect()
}

/// How the daemon gets a display in THIS session: the backend's answer, and
/// on GNOME Shell with the waywallen extension installed — enabling it.
pub fn display_setup_for(
    backend: WallpaperBackend,
    xdg_current_desktop: &str,
    waywallen_gnome_ext_installed: bool,
) -> DisplaySetup {
    if backend == WallpaperBackend::ShellExtension
        && waywallen_gnome_ext_installed
        && is_gnome_shell_session(xdg_current_desktop)
    {
        return DisplaySetup::GnomeExtension;
    }
    backend.display_setup()
}

/// The same as [`display_setup_for`], adjusted for Cinnamon with layer-shell
/// ([`cinnamon_layer_shell`]): there the scene output is the stock
/// `waywallen-layer-shell`, and it is brought up by our host output client
/// ([`DisplaySetup::RexonHost`], the «Cinnamon with layer-shell» section below).
pub fn display_setup_in(
    backend: WallpaperBackend,
    xdg_current_desktop: &str,
    waywallen_gnome_ext_installed: bool,
    cinnamon_layer_shell: bool,
) -> DisplaySetup {
    if backend == WallpaperBackend::ShellExtension && cinnamon_layer_shell && is_cinnamon_desktop(xdg_current_desktop) {
        return DisplaySetup::RexonHost;
    }
    display_setup_for(backend, xdg_current_desktop, waywallen_gnome_ext_installed)
}

// ── Cinnamon with layer-shell (Muffin 6.8+) ─────────────────────────────
//
// Live check of 15.11.2026 (Cinnamon 6.8.0, Muffin 6.8.1, Wayland): Muffin
// declares `zwlr_layer_shell_v1` v5, and the desktop background is drawn by
// the `cinnamon-background-daemon` service (D-Bus `org.Cinnamon.Background`,
// systemd unit `cinnamon-background-daemon.service`) with a surface of the
// same background layer. Hence:
//
// * Wallpapers go the same way as on wlr compositors: scenes — the stock
//   `waywallen-layer-shell` (it is started by our host output client, the
//   daemon already launches it for `x-cinnamon` from our output definition),
//   video, web and Flash — our host as a layer window. The shell extension
//   is not needed for wallpapers. The sign is the protocol (a compositor
//   global), and if there is nothing to ask the compositor with — the
//   presence of the background service (on Wayland it does not live without
//   layer-shell). The version number is not looked at.
// * In the background layer Muffin puts a surface created LATER BELOW the
//   earlier ones. The background service starts together with the shell,
//   before us, and covers our wallpaper. There is no stock way to turn off
//   its drawing: the `org.cinnamon.desktop.background` schema has no such
//   key, the service has only `Start()` and the `State`/`RenderedLayout`
//   properties on the bus, and the background manager's `disableRestart()`
//   (`js/ui/backgroundManager.js`) is an internal shell call. What remains
//   is restarting the service AFTER our surface appears: the service's new
//   surface goes under ours. [`restack_cinnamon_background`]: via systemd
//   (`systemctl --user restart` — systemd brings it up itself, the
//   background manager's "1 per 60 s" restart limit does not bother us),
//   without systemd — a signal to the name owner's process and `Start()`
//   over the bus (D-Bus activation). It is repeated for each new surface of
//   ours: monitor changes, output restarts.
// * Stopping a live wallpaper requires restoring nothing: our surface goes
//   away, under it is the same background service picture.

/// The Wayland global of the wlr-layer-shell protocol.
pub const LAYER_SHELL_GLOBAL: &str = "zwlr_layer_shell_v1";

/// Name of the Cinnamon background service on the session bus.
pub const CINNAMON_BG_BUS: &str = "org.Cinnamon.Background";
/// Object path of the background service.
pub const CINNAMON_BG_PATH: &str = "/org/Cinnamon/Background";
/// systemd unit of the background service.
pub const CINNAMON_BG_UNIT: &str = "cinnamon-background-daemon.service";

/// A Cinnamon desktop by `XDG_CURRENT_DESKTOP` (`X-Cinnamon`, `Cinnamon`).
pub fn is_cinnamon_desktop(xdg_current_desktop: &str) -> bool {
    has_desktop(xdg_current_desktop, &["x-cinnamon", "cinnamon"])
}

/// Whether the compositor declared layer-shell.
pub fn globals_have_layer_shell(globals: &[String]) -> bool {
    globals.iter().any(|g| g == LAYER_SHELL_GLOBAL)
}

/// Cinnamon on Wayland with layer-shell: wallpapers go as a layer, not via the extension.
///
/// `globals` — the compositor globals (`None` — there was nothing to ask
/// with); `bg_service_present` — whether the Cinnamon background service
/// exists (a D-Bus activation file): on Wayland it draws only via
/// layer-shell, so without a compositor answer it is the same sign.
pub fn cinnamon_layer_shell(
    xdg_current_desktop: &str,
    session_type: &str,
    wayland_display: &str,
    globals: Option<&[String]>,
    bg_service_present: bool,
) -> bool {
    if !is_cinnamon_desktop(xdg_current_desktop) || !is_wayland_session(session_type, wayland_display) {
        return false;
    }
    match globals {
        Some(g) => globals_have_layer_shell(g),
        None => bg_service_present,
    }
}

/// Activation files of the Cinnamon background service in the
/// `dbus-1/services` directories (`$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`,
/// empty — `/usr/local/share:/usr/share`).
pub fn cinnamon_bg_service_files(xdg_data_home: &str, home: &str, xdg_data_dirs: &str) -> Vec<String> {
    let user = if xdg_data_home.trim().is_empty() {
        format!("{}/.local/share", home.trim_end_matches('/'))
    } else {
        xdg_data_home.trim_end_matches('/').to_string()
    };
    let system = if xdg_data_dirs.trim().is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
    std::iter::once(user.as_str())
        .chain(system.split(':').map(|d| d.trim_end_matches('/')).filter(|d| !d.is_empty()))
        .map(|base| format!("{base}/dbus-1/services/{CINNAMON_BG_BUS}.service"))
        .collect()
}

/// The "Cinnamon with layer-shell" session fact set by a live process
/// ([`set_cinnamon_layer_shell`]). 0 — not set (treated as "no": that is how
/// checks and processes that did not ask the compositor behave), 1 — no, 2 — yes.
static CINNAMON_LAYER_SHELL_FACT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Remember for the process whether this session is Cinnamon with layer-shell.
/// Called by live processes (the Rexon window, the wallpaper host) at startup.
pub fn set_cinnamon_layer_shell(on: bool) {
    CINNAMON_LAYER_SHELL_FACT.store(if on { 2 } else { 1 }, std::sync::atomic::Ordering::SeqCst);
}

/// The "Cinnamon with layer-shell" session fact; not set — `false`.
pub fn cinnamon_layer_shell_fact() -> bool {
    CINNAMON_LAYER_SHELL_FACT.load(std::sync::atomic::Ordering::SeqCst) == 2
}

/// Find out the fact on a live session and remember it. `globals` — the
/// compositor's answer (the core has no Wayland client of its own — the caller asks).
pub fn init_cinnamon_layer_shell_live(globals: Option<&[String]>) -> bool {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    let service = || {
        cinnamon_bg_service_files(&env("XDG_DATA_HOME"), &env("HOME"), &env("XDG_DATA_DIRS"))
            .iter()
            .any(|p| std::path::Path::new(p).is_file())
    };
    let on = cinnamon_layer_shell(
        &env("XDG_CURRENT_DESKTOP"),
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        globals,
        is_cinnamon_desktop(&env("XDG_CURRENT_DESKTOP")) && service(),
    );
    set_cinnamon_layer_shell(on);
    if on {
        crate::log_info!("cinnamon-bg", "Cinnamon с layer-shell: живые обои слоем стола, не расширением оболочки");
    }
    on
}

/// Observations and actions for [`restack_cinnamon_background`].
pub trait CinnamonBgIo {
    /// Run a command: stdout on code 0, otherwise `None`.
    fn run(&self, argv: &[String]) -> Option<String>;
    /// Wait.
    fn sleep_ms(&self, ms: u64);
}

/// What came out of [`restack_cinnamon_background`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgRestack {
    /// The background service is not on the bus — nothing to restack.
    NotRunning,
    /// The service was restarted, its surface is now under ours.
    Restarted,
    /// The restart failed.
    Failed,
}

fn svec(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// The bus's `NameHasOwner` — `(true,)`/`(false,)`.
pub fn bus_name_has_owner_argv(name: &str) -> Vec<String> {
    svec(&[
        "gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus",
        "--method", "org.freedesktop.DBus.NameHasOwner", name,
    ])
}

/// PID of the name owner's process — `(uint32 1234,)`.
pub fn bus_name_pid_argv(name: &str) -> Vec<String> {
    svec(&[
        "gdbus", "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus",
        "--method", "org.freedesktop.DBus.GetConnectionUnixProcessID", name,
    ])
}

/// The background service's `Start()`: on a running one — no action, on a
/// missing one — D-Bus activation.
pub fn cinnamon_bg_start_argv() -> Vec<String> {
    svec(&[
        "gdbus", "call", "--session", "--dest", CINNAMON_BG_BUS, "--object-path", CINNAMON_BG_PATH, "--method",
        "org.Cinnamon.Background.Start",
    ])
}

/// A `gdbus` answer — `(true,)`.
pub fn parse_gdbus_bool(out: &str) -> bool {
    out.trim().trim_start_matches('(').starts_with("true")
}

/// A number from a `gdbus` answer of the form `(uint32 1234,)`.
pub fn parse_gdbus_uint(out: &str) -> Option<u32> {
    out.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).last()?.parse().ok()
}

/// How long to wait for the service name to go away and come back (in 100 ms steps).
const BG_WAIT_STEPS: u32 = 50;

/// Restart the Cinnamon background service so that its surface goes under ours.
pub fn restack_cinnamon_background(io: &dyn CinnamonBgIo) -> BgRestack {
    let owner = |io: &dyn CinnamonBgIo| io.run(&bus_name_has_owner_argv(CINNAMON_BG_BUS)).is_some_and(|o| parse_gdbus_bool(&o));
    if !owner(io) {
        return BgRestack::NotRunning;
    }
    let unit_active = io
        .run(&svec(&["systemctl", "--user", "is-active", CINNAMON_BG_UNIT]))
        .is_some_and(|o| o.trim() == "active");
    if unit_active {
        // Type=dbus: `restart` returns when the service has taken the name again.
        return if io.run(&svec(&["systemctl", "--user", "restart", CINNAMON_BG_UNIT])).is_some() {
            BgRestack::Restarted
        } else {
            BgRestack::Failed
        };
    }
    let Some(pid) = io.run(&bus_name_pid_argv(CINNAMON_BG_BUS)).and_then(|o| parse_gdbus_uint(&o)) else {
        return BgRestack::Failed;
    };
    if io.run(&svec(&["kill", "-TERM", &pid.to_string()])).is_none() {
        return BgRestack::Failed;
    }
    for _ in 0..BG_WAIT_STEPS {
        if !owner(io) {
            break;
        }
        io.sleep_ms(100);
    }
    // Our own `Start()`: the background manager gives up after the restart
    // limit, and D-Bus activation brings the service up even without it.
    io.run(&cinnamon_bg_start_argv());
    for _ in 0..BG_WAIT_STEPS {
        if owner(io) {
            return BgRestack::Restarted;
        }
        io.sleep_ms(100);
    }
    BgRestack::Failed
}

/// Live observations for [`restack_cinnamon_background`].
pub struct LiveCinnamonBgIo;

impl CinnamonBgIo for LiveCinnamonBgIo {
    fn run(&self, argv: &[String]) -> Option<String> {
        crate::splash_exec::run_capture(argv, 20.0).filter(|o| o.rc == 0).map(|o| o.stdout)
    }
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// A Cinnamon session on Wayland, by the process environment.
pub fn cinnamon_wayland_session_live() -> bool {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    is_cinnamon_desktop(&env("XDG_CURRENT_DESKTOP")) && is_wayland_session(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"))
}

/// How long after the request to restack the background when nothing is
/// known about our surface being shown (a monitor was connected, the
/// background service came up again): the surface needs to wait for the
/// compositor's configure and a frame.
pub const BG_RESTACK_DELAY_MS: u64 = 2500;

/// A fallback deadline for our own surface whose showing the host will
/// report itself ([`restack_cinnamon_background_shown`]): the "first frame on
/// screen" notice came — restack right away; it did not come (the movie does
/// not decode, the page is silent) — by this deadline, so that the
/// background service does not cover the window forever.
pub const BG_RESTACK_FALLBACK_MS: u64 = 6000;

/// A delay after the "first frame on screen" notice: for the compositor to
/// process the frame commit before the background service goes away and
/// comes back under us.
pub const BG_RESTACK_SHOWN_MS: u64 = 50;

/// For how long after a background service restart (by any Rexon process)
/// its reappearance on the bus counts as ours rather than its own restart.
pub const BG_OWN_RESTART_WINDOW_MS: u64 = 15_000;

/// No more than this many background service restarts per
/// [`BG_RESTACK_WINDOW_MS`] per process: a service crashing in a loop must
/// not drag us into endless restarting (the Cinnamon background manager
/// gives up after the very first one in that case — `RESTART_LIMIT`).
pub const BG_RESTACK_LIMIT: usize = 3;
/// The counting window of [`BG_RESTACK_LIMIT`].
pub const BG_RESTACK_WINDOW_MS: u64 = 120_000;

/// The "Rexon restarted the background service" mark — shared by all Rexon
/// processes of the session (the window, wallpaper hosts, the output client):
/// bus watchers see its time so as not to take someone else's (our own)
/// restart for a service crash.
pub fn bg_restack_stamp_path(xdg_runtime_dir: &str) -> String {
    format!("{}/rexon-cinnamon-bg-restack.stamp", xdg_runtime_dir.trim_end_matches('/'))
}

/// The restack lock next to the mark: the background service is restarted in
/// the session by one Rexon process at a time.
///
/// A live case of 15.11.2026: the Rexon window and the video wallpaper host
/// ordered a restack almost simultaneously; the window restarted the service,
/// and the host in the same milliseconds asked the bus, found the name gone
/// and wrote to the journal «there is no Cinnamon background service —
/// nothing to restack». Under the lock the second one waits for the end of
/// the first restart and sees from the mark that there is no need to repeat.
pub fn bg_restack_lock_path(xdg_runtime_dir: &str) -> String {
    format!("{}.lock", bg_restack_stamp_path(xdg_runtime_dir))
}

/// The restart mark: when it started and when it ended (ms since the epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgRestackStamp {
    pub started: u64,
    /// `None` — the restart is still in progress (or the mark has the old form — one number).
    pub finished: Option<u64>,
}

impl BgRestackStamp {
    /// The last known restart moment.
    pub fn latest(&self) -> u64 {
        self.finished.unwrap_or(self.started).max(self.started)
    }

    /// The mark text: «start» or «start end».
    pub fn to_text(&self) -> String {
        match self.finished {
            Some(end) => format!("{} {end}", self.started),
            None => self.started.to_string(),
        }
    }
}

/// Parse the mark text ([`BgRestackStamp::to_text`]; a single number is a
/// mark of the old form, its moment counts as the start).
pub fn parse_bg_restack_stamp(text: &str) -> Option<BgRestackStamp> {
    let mut parts = text.split_whitespace();
    let started = parts.next()?.parse().ok()?;
    let finished = parts.next().and_then(|s| s.parse().ok());
    Some(BgRestackStamp { started, finished })
}

/// Whether the service may be restarted once more: `history` — moments of
/// previous restarts by this process (ms), `now` — now.
pub fn bg_restack_allowed(history: &[u64], now: u64) -> bool {
    history.iter().filter(|&&t| now.saturating_sub(t) < BG_RESTACK_WINDOW_MS).count() < BG_RESTACK_LIMIT
}

/// The background service is on the bus again: whether this is our restart
/// (the mark is fresher than [`BG_OWN_RESTART_WINDOW_MS`]) — then there is
/// nothing to restack.
pub fn bg_owner_change_is_ours(stamp_ms: Option<u64>, now: u64) -> bool {
    stamp_ms.is_some_and(|t| now.saturating_sub(t) < BG_OWN_RESTART_WINDOW_MS)
}

/// A restack ordered at `requested` has already been done by another Rexon
/// process if its restart STARTED (`started_ms`) no earlier than
/// `requested + slack_ms`: it put the service under everything that was on
/// screen by then. For a "first frame on screen" order `slack_ms` is zero:
/// the surface is already shown, any restart after it will do.
pub fn bg_restack_done_by_other(started_ms: Option<u64>, requested: u64, slack_ms: u64) -> bool {
    started_ms.is_some_and(|t| t >= requested.saturating_add(slack_ms))
}

/// A restack order waiting for its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgRestackOrder {
    /// When to execute (ms since the epoch).
    pub due: u64,
    /// A restart started no earlier than `requested + slack` (ms since the
    /// epoch) has already covered our surface ([`bg_restack_done_by_other`]).
    pub requested: u64,
    pub slack: u64,
}

/// Merge a new request with the waiting order.
///
/// * A deadline request (`shown == false`, `delay_ms` from `now`) only
///   postpones execution: several requests in a row — a window per monitor,
///   bringing up the output — merge into one restart after the last one.
/// * The "first frame on screen" notice (`shown == true`) pulls the deadline
///   in to `now + BG_RESTACK_SHOWN_MS` and replaces the fallback one: there
///   is no reason to wait longer, the surface is already shown.
pub fn merge_bg_restack_order(pending: Option<BgRestackOrder>, now: u64, delay_ms: u64, shown: bool) -> BgRestackOrder {
    if shown {
        return BgRestackOrder { due: now + BG_RESTACK_SHOWN_MS, requested: now, slack: 0 };
    }
    let due = now + delay_ms;
    match pending {
        Some(p) => BgRestackOrder { due: p.due.max(due), ..p },
        None => BgRestackOrder { due, requested: now, slack: delay_ms },
    }
}

fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn bg_runtime_dir_live() -> Option<String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    (!runtime.is_empty()).then_some(runtime)
}

/// The restart mark, `None` — there is no mark.
pub fn bg_restack_stamp_live() -> Option<BgRestackStamp> {
    let path = bg_restack_stamp_path(&bg_runtime_dir_live()?);
    parse_bg_restack_stamp(&std::fs::read_to_string(path).ok()?)
}

/// The last restart moment (ms since the epoch), `None` — there is no mark.
pub fn bg_restack_stamp_ms_live() -> Option<u64> {
    bg_restack_stamp_live().map(|s| s.latest())
}

fn bg_restack_stamp_write_live(stamp: BgRestackStamp) {
    if let Some(runtime) = bg_runtime_dir_live() {
        let _ = std::fs::write(bg_restack_stamp_path(&runtime), stamp.to_text());
    }
}

/// Take the restack lock ([`bg_restack_lock_path`]), waiting for it.
/// `None` — there is no lock (no session directory) — we go without it.
fn bg_restack_lock_live() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let path = bg_restack_lock_path(&bg_runtime_dir_live()?);
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    // SAFETY: a live file descriptor; the lock is released by closing the file.
    (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0).then_some(file)
}

/// Defer the Cinnamon background restack: several requests in a row (a window
/// per monitor, bringing up the output) merge into one service restart
/// [`BG_RESTACK_DELAY_MS`] after the last one. Outside Cinnamon on Wayland —
/// nothing.
pub fn restack_cinnamon_background_soon(reason: &str) {
    restack_cinnamon_background_after(reason, BG_RESTACK_DELAY_MS);
}

/// The same with an explicit delay (a Wallpaper Engine scene needs more: its
/// surface gets the first frame when the daemon has already brought up the
/// renderer; for our host's own surface — the fallback deadline
/// [`BG_RESTACK_FALLBACK_MS`]).
pub fn restack_cinnamon_background_after(reason: &str, delay_ms: u64) {
    schedule_bg_restack(reason, delay_ms, false);
}

/// Our surface showed its first frame (video — the movie's first frame): put
/// the background service under it now, without waiting for the fallback deadline.
pub fn restack_cinnamon_background_shown(reason: &str) {
    schedule_bg_restack(reason, 0, true);
}

fn schedule_bg_restack(reason: &str, delay_ms: u64, shown: bool) {
    use std::sync::Mutex;
    use std::time::Duration;
    /// The waiting order.
    static PENDING: Mutex<Option<BgRestackOrder>> = Mutex::new(None);
    static HISTORY: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    /// The execution thread is alive (the order may already be in progress, but the thread not yet gone).
    static WORKER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    use std::sync::atomic::Ordering;
    if !cinnamon_wayland_session_live() {
        return;
    }
    {
        let mut slot = PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(merge_bg_restack_order(*slot, unix_ms_now(), delay_ms, shown));
    }
    if shown {
        crate::log_info!("cinnamon-bg", "фон Cinnamon укладываем под наши обои сейчас: {reason} — первый кадр на экране");
    } else {
        crate::log_info!("cinnamon-bg", "фон Cinnamon уложим под наши обои: {reason}");
    }
    if WORKER.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("rexon-cinnamon-bg".into()).spawn(move || loop {
        let order = {
            let mut slot = PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match *slot {
                Some(o) if o.due > unix_ms_now() => Ok(o.due.saturating_sub(unix_ms_now())),
                Some(o) => {
                    *slot = None;
                    Err(o)
                }
                None => {
                    WORKER.store(false, Ordering::SeqCst);
                    return;
                }
            }
        };
        let order = match order {
            // The sleep step is short: the "first frame" notice may pull the deadline in.
            Ok(wait) => {
                std::thread::sleep(Duration::from_millis(wait.min(50)));
                continue;
            }
            Err(order) => order,
        };
        run_bg_restack(order, &HISTORY);
    });
    if let Err(e) = spawned {
        crate::log_warn!("cinnamon-bg", "перестановка фона не запущена: {e}");
        if let Ok(mut slot) = PENDING.lock() {
            *slot = None;
        }
        WORKER.store(false, Ordering::SeqCst);
    }
}

/// Execute a matured order: under the session lock, with protection against looping.
fn run_bg_restack(order: BgRestackOrder, history: &std::sync::Mutex<Vec<u64>>) {
    // Under the lock: another Rexon process restarting the service right now
    // will finish first, and its mark will be visible.
    let _lock = bg_restack_lock_live();
    if bg_restack_done_by_other(bg_restack_stamp_live().map(|s| s.started), order.requested, order.slack) {
        crate::log_info!("cinnamon-bg", "службу фона уже перезапустил другой процесс Rexon — повторять незачем");
        return;
    }
    let now = unix_ms_now();
    {
        let mut history = history.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !bg_restack_allowed(&history, now) {
            crate::log_warn!(
                "cinnamon-bg",
                "служба фона Cinnamon перезапускается слишком часто — больше не трогаем её ({BG_RESTACK_LIMIT} раза за {} с)",
                BG_RESTACK_WINDOW_MS / 1000
            );
            return;
        }
        history.retain(|&t| now.saturating_sub(t) < BG_RESTACK_WINDOW_MS);
        history.push(now);
    }
    bg_restack_stamp_write_live(BgRestackStamp { started: now, finished: None });
    let res = restack_cinnamon_background(&LiveCinnamonBgIo);
    bg_restack_stamp_write_live(BgRestackStamp { started: now, finished: Some(unix_ms_now()) });
    match res {
        BgRestack::Restarted => {
            crate::log_info!("cinnamon-bg", "служба фона Cinnamon перезапущена — её картинка под нашими обоями")
        }
        BgRestack::NotRunning => crate::log_info!("cinnamon-bg", "службы фона Cinnamon нет — переставлять нечего"),
        BgRestack::Failed => {
            crate::log_warn!("cinnamon-bg", "службу фона Cinnamon перезапустить не вышло — её картинка может закрыть обои")
        }
    }
}

/// The bus watcher saw the background service take the name again. Not our
/// restart (the service crashed, the background manager or D-Bus activation
/// brought it up) — its new surface lay on top of our wallpaper: restack again.
pub fn on_cinnamon_bg_owner_appeared_live() {
    if bg_owner_change_is_ours(bg_restack_stamp_ms_live(), unix_ms_now()) {
        return;
    }
    restack_cinnamon_background_soon("служба фона Cinnamon поднялась заново");
}

/// Watch the bus: the Cinnamon background service took the name again not by
/// our restart (crashed and came up, the background manager restarted it) —
/// its new surface lay on top of our wallpaper, restack
/// ([`on_cinnamon_bg_owner_appeared_live`]). One thread per process; outside
/// Cinnamon on Wayland — nothing. Called by processes whose surface lies in
/// the background layer: the wallpaper host as a layer and the scene output client.
pub fn spawn_cinnamon_bg_owner_watch_live() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if !cinnamon_wayland_session_live() {
        return;
    }
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new().name("rexon-cinnamon-bg-watch".into()).spawn(|| {
            let watch = || -> zbus::Result<()> {
                let conn = zbus::blocking::Connection::session()?;
                let proxy = zbus::blocking::fdo::DBusProxy::new(&conn)?;
                for signal in proxy.receive_name_owner_changed_with_args(&[(0, CINNAMON_BG_BUS)])? {
                    if signal.args()?.new_owner().is_some() {
                        on_cinnamon_bg_owner_appeared_live();
                    }
                }
                Ok(())
            };
            if let Err(e) = watch() {
                crate::log_warn!("cinnamon-bg", "слежка за службой фона Cinnamon оборвалась: {e}");
            }
        });
        if let Err(e) = spawned {
            crate::log_warn!("cinnamon-bg", "слежка за службой фона Cinnamon не запущена: {e}");
        }
    });
}

/// The host output client on Wayland with layer-shell does not draw itself
/// but brings up waywallen's stock output client — this binary.
pub const LAYER_SHELL_CLIENT_BIN: &str = "waywallen-layer-shell";

/// Where to look for `waywallen-layer-shell`, in order: next to the parent
/// (the daemon launches the host itself; with an AppImage the daemon lives in
/// the mount point, and the output client lies there too), next to live
/// daemon processes, next to the distribution package's daemon. The last
/// chance is a `PATH` lookup by the executor ([`LAYER_SHELL_CLIENT_BIN`]).
pub fn layer_shell_client_candidates(parent_exe: Option<&str>, daemon_exes: &[String]) -> Vec<String> {
    let sibling = |exe: &str| {
        std::path::Path::new(exe)
            .parent()
            .map(|d| d.join(LAYER_SHELL_CLIENT_BIN).to_string_lossy().into_owned())
    };
    let is_daemon = |exe: &str| {
        std::path::Path::new(exe)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "waywallen")
    };
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: Option<String>| {
        if let Some(p) = p {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    };
    if let Some(exe) = parent_exe.filter(|e| is_daemon(e)) {
        push(sibling(exe));
    }
    for exe in daemon_exes.iter().filter(|e| is_daemon(e)) {
        push(sibling(exe));
    }
    push(sibling(crate::we_client::SYSTEM_DAEMON));
    out
}

/// The output client command line for the daemon socket.
pub fn layer_shell_client_argv(bin: &str, socket: &str) -> Vec<String> {
    let mut argv = vec![bin.to_string()];
    if !socket.is_empty() {
        argv.push("--socket".to_string());
        argv.push(socket.to_string());
    }
    argv.push("--name".to_string());
    argv.push("rexon".to_string());
    argv
}

/// The "Rexon released the scene output" mark: our own wallpapers (picture,
/// video…) on Cinnamon with layer-shell take down the `waywallen-layer-shell`
/// surface, otherwise it would cover them; applying a scene removes the mark.
/// It lies in `$XDG_STATE_HOME/rexon/` — survives re-login: the person's last
/// choice remains their own wallpaper, not the scene.
pub fn display_off_marker_path(xdg_state_home: &str, home: &str) -> String {
    let base = if xdg_state_home.trim().is_empty() {
        format!("{}/.local/state", home.trim_end_matches('/'))
    } else {
        xdg_state_home.trim_end_matches('/').to_string()
    };
    format!("{base}/rexon/waywallen-display.off")
}

/// The mark's path on a live session.
pub fn display_off_marker_live() -> String {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    display_off_marker_path(&env("XDG_STATE_HOME"), &env("HOME"))
}

/// Release (`off = true`) or bring back the scene output on Cinnamon with
/// layer-shell. True — the mark was changed.
pub fn set_layer_shell_display_off_live(off: bool) -> bool {
    let path = display_off_marker_live();
    let p = std::path::Path::new(&path);
    if off == p.exists() {
        return false;
    }
    let done = if off {
        p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(p, b"").is_ok()
    } else {
        std::fs::remove_file(p).is_ok()
    };
    if done {
        crate::log_info!(
            "cinnamon-bg",
            "вывод сцен Wallpaper Engine {}",
            if off { "отпущен ради своих обоев" } else { "возвращён" }
        );
    }
    done
}

/// What the host output client should do on Wayland.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandHostRole {
    /// Wait: nothing to output with, or it is not ours to do.
    Idle,
    /// Bring up and watch the stock output client with this command line.
    Delegate(Vec<String>),
}

/// The host output client's decision on Wayland: a compositor with
/// layer-shell and a found `waywallen-layer-shell` — bring it up (on any
/// desktop from [`REXON_DISPLAY_DESKTOPS`]: Cinnamon 6.8, XFCE/LXQt on
/// labwc); otherwise wait.
pub fn wayland_host_role(globals: &[String], client_bin: Option<&str>, socket: &str) -> WaylandHostRole {
    match client_bin {
        Some(bin) if globals_have_layer_shell(globals) => WaylandHostRole::Delegate(layer_shell_client_argv(bin, socket)),
        _ => WaylandHostRole::Idle,
    }
}

/// The pause before bringing a crashed output client up again: 2 s, doubled
/// for each crash in a row, no more than 30 s (as with the daemon, only with
/// a higher ceiling — the client usually crashes from a compositor refusal).
pub fn layer_shell_restart_delay_ms(crashes_in_row: u32) -> u64 {
    (2000u64 << crashes_in_row.min(4)).min(30_000)
}

/// What came out of [`set_waywallen_gnome_display`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnomeDisplaySwitch {
    /// Nothing to touch: not GNOME Shell, no extension or the shell did not
    /// answer. Silently — this is not the person's mistake.
    Skipped,
    /// The extension is already in the required state.
    Already,
    /// Switched.
    Switched,
    /// Switching failed (or enabling was useless: an error, the wrong version,
    /// the shell has not loaded it yet).
    Failed,
}

/// Observations and actions for [`set_waywallen_gnome_display`].
pub trait GnomeExtIo {
    /// An environment variable (empty — none).
    fn env(&self, name: &str) -> String;
    /// Whether the directory exists.
    fn is_dir(&self, path: &str) -> bool;
    /// Run a command: stdout on code 0, otherwise `None`.
    fn run(&self, argv: &[String]) -> Option<String>;
}

/// Directory of the installed waywallen extension, if there is one.
pub fn waywallen_gnome_ext_dir(io: &dyn GnomeExtIo) -> Option<String> {
    gnome_extension_dirs(
        WAYWALLEN_GNOME_EXT_ID,
        &io.env("XDG_DATA_HOME"),
        &io.env("HOME"),
        &io.env("XDG_DATA_DIRS"),
    )
    .into_iter()
    .find(|d| io.is_dir(d))
}

/// Enable (`on`) or disable the waywallen output extension in GNOME.
///
/// The state is asked from the shell itself
/// ([`crate::wallpaper_engine::gnome_ext_info_argv`]): no answer — there is
/// no GNOME Shell session, we touch nothing.
pub fn set_waywallen_gnome_display(io: &dyn GnomeExtIo, on: bool) -> GnomeDisplaySwitch {
    use crate::wallpaper_engine::{gnome_ext_info_argv, parse_shell_ext_state, ShellExtState as S};
    if !is_gnome_shell_session(&io.env("XDG_CURRENT_DESKTOP")) {
        return GnomeDisplaySwitch::Skipped;
    }
    if waywallen_gnome_ext_dir(io).is_none() {
        return GnomeDisplaySwitch::Skipped;
    }
    let state = parse_shell_ext_state("gnome", io.run(&gnome_ext_info_argv(WAYWALLEN_GNOME_EXT_ID)).as_deref());
    match (state, on) {
        (S::Unknown, _) => return GnomeDisplaySwitch::Skipped,
        (S::Enabled, true) => return GnomeDisplaySwitch::Already,
        // Not working — and does not cover the desktop either.
        (S::Disabled | S::Error | S::OutOfDate | S::NotLoaded, false) => return GnomeDisplaySwitch::Already,
        _ => {}
    }
    // The shell itself (`EnableExtension`/`DisableExtension`): it also edits
    // both settings lists. Our own edit of `enabled-extensions` after
    // `gnome-extensions` must not be done here — the shell writes the lists
    // later, and two racing writes left the extension disabled while the
    // settings said "enabled" (live 09.11.2026). The install script
    // (`gnome_extension_enable_script`) will not do either: it recompiles the
    // foreign extension's schemas, and under disk load that takes 5+ s.
    // The bus did not answer — the fallback path: the settings list without
    // losing foreign uuids.
    let ok = io.run(&gnome_ext_toggle_argv(WAYWALLEN_GNOME_EXT_ID, on)).is_some_and(|out| out.contains("true"))
        || {
            use crate::de_customization::{gsettings_list_add_script, gsettings_list_remove_script};
            let (schema, key, uuid) = ("org.gnome.shell", "enabled-extensions", WAYWALLEN_GNOME_EXT_ID);
            let script = if on {
                gsettings_list_add_script(schema, key, uuid, uuid)
            } else {
                gsettings_list_remove_script(schema, key, uuid)
            };
            io.run(&["sh".to_string(), "-c".to_string(), script]).is_some()
        };
    match (ok, state) {
        // Enabled, but there will be no display now: tell the hold-back "it did not
        // work" so it does not wait in vain.
        (true, S::Error | S::OutOfDate | S::NotLoaded) if on => GnomeDisplaySwitch::Failed,
        (true, _) => GnomeDisplaySwitch::Switched,
        (false, _) => GnomeDisplaySwitch::Failed,
    }
}

/// GNOME Shell's `EnableExtension`/`DisableExtension`; the answer is `(true,)`.
pub fn gnome_ext_toggle_argv(uuid: &str, on: bool) -> Vec<String> {
    let method = if on {
        "org.gnome.Shell.Extensions.EnableExtension"
    } else {
        "org.gnome.Shell.Extensions.DisableExtension"
    };
    [
        "gdbus", "call", "--session", "--dest", "org.gnome.Shell", "--object-path", "/org/gnome/Shell",
        "--method", method, uuid,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Live observations for [`set_waywallen_gnome_display`].
pub struct LiveGnomeExtIo;

impl GnomeExtIo for LiveGnomeExtIo {
    fn env(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }
    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }
    fn run(&self, argv: &[String]) -> Option<String> {
        crate::splash_exec::run_capture(argv, 20.0).filter(|o| o.rc == 0).map(|o| o.stdout)
    }
}

/// The same on a live session, with a journal entry when something was switched.
pub fn set_waywallen_gnome_display_live(on: bool) -> GnomeDisplaySwitch {
    let res = set_waywallen_gnome_display(&LiveGnomeExtIo, on);
    let what = if on { "включено" } else { "выключено" };
    match res {
        GnomeDisplaySwitch::Switched => {
            crate::log_info!("gnome-display", "расширение вывода waywallen {what} ({WAYWALLEN_GNOME_EXT_ID})")
        }
        GnomeDisplaySwitch::Failed => crate::log_warn!(
            "gnome-display",
            "расширение вывода waywallen не {what}: {WAYWALLEN_GNOME_EXT_ID}"
        ),
        GnomeDisplaySwitch::Skipped | GnomeDisplaySwitch::Already => {}
    }
    res
}

/// Whether the waywallen output extension is installed (a live file system
/// probe, without the bus — fit for frequent calls).
pub fn waywallen_gnome_ext_installed_live() -> bool {
    waywallen_gnome_ext_dir(&LiveGnomeExtIo).is_some()
}

/// The daemon's output definition name (the `name` field, visible in its
/// journal and in `display_backend_status`).
pub const REXON_DISPLAY_NAME: &str = "rexon-x11";

/// The wallpaper host mode in which it is the daemon's output client.
pub const REXON_DISPLAY_HOST_ARGS: &[&str] = &["--mode", "waywallen"];

/// Desktops (`XDG_CURRENT_DESKTOP` tokens, case-insensitive) on which the
/// daemon brings up our host. Only desktops without waywallen's own output:
/// `kde` is under the daemon's hard rule, `gnome` belongs to its extension,
/// wlr to `waywallen-layer-shell`. Some of these desktops also happen on
/// Wayland (XFCE and LXQt on labwc, Cinnamon Wayland) — there the host shows
/// nothing and only waits ([`rexon_host_should_idle`]), so that the daemon
/// does not restart it in a loop.
pub const REXON_DISPLAY_DESKTOPS: &[&str] = &[
    "mate",
    "xfce",
    "x-cinnamon",
    "cinnamon",
    "budgie",
    "budgie-desktop",
    "lxqt",
    "lxde",
    "pantheon",
    "deepin",
    "unity",
    "enlightenment",
];

/// Definition priority: above the built-in `x11` (50; the daemon with
/// waywallen#354 brings up `waywallen-x11` itself on MATE, Cinnamon and
/// Xfce). Otherwise on these desktops the daemon would choose its own output:
/// without the binary next to it there is no wallpaper at all, with the
/// binary its window goes under the opaque Caja. The desktop lists of
/// `layer-shell`, `kde` and `gnome` do not overlap — 60 takes nothing from them.
pub const REXON_DISPLAY_PRIORITY: i32 = 60;

/// Where the output definition for the daemon lies: the daemon's user
/// `displays` directory (`$XDG_DATA_HOME/waywallen/displays`). Any daemon
/// reads it — the AppImage one bundled in Rexon and the distribution package.
pub fn rexon_display_manifest_path(xdg_data_home: &str) -> String {
    format!("{}/waywallen/displays/{REXON_DISPLAY_NAME}.toml", xdg_data_home.trim_end_matches('/'))
}

/// Escape a string for TOML (a basic quoted string).
fn toml_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The output definition text (the daemon's `DisplayManifest`: the `[display]` section).
/// The daemon runs `bin --socket <path> <extra_args>` and watches it itself.
pub fn rexon_display_manifest(host_bin: &str) -> String {
    let list = |items: &[&str]| items.iter().map(|s| toml_str(s)).collect::<Vec<_>>().join(", ");
    format!(
        "# Rexon: живые обои на рабочих столах X11, где у waywallen нет своего\n\
         # вывода. Файл пишет Rexon сам; правки руками он перезапишет.\n\
         [display]\n\
         name = {}\n\
         bin = {}\n\
         extra_args = [{}]\n\
         de = [{}]\n\
         priority = {REXON_DISPLAY_PRIORITY}\n\
         spawn = \"daemon\"\n",
        toml_str(REXON_DISPLAY_NAME),
        toml_str(host_bin),
        list(REXON_DISPLAY_HOST_ARGS),
        list(REXON_DISPLAY_DESKTOPS),
    )
}

/// Whether to rewrite the definition: no file or it is not the right one
/// (another host path after reinstall, an old format).
pub fn rexon_display_manifest_stale(current: Option<&str>, host_bin: &str) -> bool {
    current != Some(rexon_display_manifest(host_bin).as_str())
}

/// The wallpaper host next to the running Rexon binary (the developer tree:
/// `target*/release/`), otherwise a `PATH` lookup by the executor (the
/// package puts the host into `/usr/bin`).
pub fn rexon_display_host_sibling(self_exe: &str) -> Option<String> {
    let dir = std::path::Path::new(self_exe).parent()?;
    Some(dir.join(crate::wallpaper_engine::WALLPAPER_HOST_BINARY).to_string_lossy().into_owned())
}

/// The command line with which Rexon launches the host output client itself —
/// the same as the daemon's from the output definition
/// (`bin --socket <path> <extra_args>`): one switch parser in the host for
/// both paths.
pub fn rexon_display_host_argv(host_bin: &str, socket: &str) -> Vec<String> {
    let mut argv = vec![host_bin.to_string()];
    if !socket.is_empty() {
        argv.push("--socket".to_string());
        argv.push(socket.to_string());
    }
    argv.extend(REXON_DISPLAY_HOST_ARGS.iter().map(|s| s.to_string()));
    argv
}

/// The "another host instance is already showing the output" lock.
pub fn rexon_host_lock_path(xdg_runtime_dir: &str) -> String {
    format!("{}/rexon-waywallen-display.lock", xdg_runtime_dir.trim_end_matches('/'))
}

/// Whether the host output client should only wait, showing nothing.
///
/// Yes on Wayland: X11 windows there would lie over the desktop as ordinary
/// windows (via XWayland), and the background layer is other outputs'
/// business. No `DISPLAY` — the window has nowhere to be at all.
pub fn rexon_host_should_idle(session_type: &str, wayland_display: &str, display: &str) -> bool {
    is_wayland_session(session_type, wayland_display) || display.trim().is_empty()
}

/// Backend name for the journal and the bridge (stable strings, not for the UI).
pub fn backend_id(backend: WallpaperBackend) -> &'static str {
    match backend {
        WallpaperBackend::KdePlugin => "kde-plugin",
        WallpaperBackend::DaemonLayerShell => "daemon-layer-shell",
        WallpaperBackend::ShellExtension => "shell-extension",
        WallpaperBackend::X11Subprocess => "x11-subprocess",
        WallpaperBackend::Unsupported => "unsupported",
    }
}

// ── desktop icons under a live wallpaper (X11) ──────────────────────────
//
// Defect of 31.10.2026 (MATE 1.28, X11): live wallpaper «Apply» works, but
// the host window goes above Caja's icons window — the icons are not
// visible. The window order is decided by the host itself
// (`wallpaper_mmap::x11_restack_goal`): under the icons when the icons
// window background is transparent, above them when it is opaque. Here is
// what to do with the desktop on apply and what to restore later:
//
// * Caja with the `org.mate.caja.desktop transparent-background` key (our
//   mate-desktop/caja#1916; whether the key exists is asked from the schema,
//   not by version): the key is turned on, the Marco compositor too if it is
//   off (without it window alpha is not blended), the desktop window is
//   recreated (Caja reads the key when creating the window). The previous
//   values are recorded and restored by the «Stop live wallpaper» button in
//   the settings and by rolling back a failed scene apply. Stopping a scene
//   in the Wallpaper Engine tab does not touch the key: without a wallpaper
//   window under it, Caja's transparent desktop shows the regular desktop
//   picture (the root window), verified on the test bench.
// * Caja without the key and xfdesktop draw the background opaque — the
//   icons are hidden under the live wallpaper while it runs, and the person
//   is told so in words. The icons themselves are NOT turned off: the
//   wallpaper window is gone — the icons are visible again.
// * nemo-desktop is always transparent (an RGBA visual), but on startup it
//   refuses to draw the desktop itself («Desktop already managed») if the
//   lowest "desktop" type window in `_NET_CLIENT_LIST_STACKING` is a foreign
//   one — and that is the wallpaper window if it was shown before
//   nemo-desktop (session login, Nemo restart). Exceptions are the
//   `org.nemo.desktop ignored-desktop-handlers` key: a `WM_CLASS` (res_class)
//   substring, case-insensitive (Nemo 6.6 `desktop_handler_is_ignored`); the
//   class of all wallpaper hosts starts with `Rexon`. Where this schema
//   exists, Rexon appends `rexon` there without touching foreign values, and
//   records the previous value and restores it with the same «Stop live
//   wallpaper».
//
// ⚠️ Recreating the desktop window means toggling `org.mate.background
// show-desktop-icons` "off → on", not `caja -q`: quitting Caja would close
// the file manager's open windows, and under mate-session Caja also
// restarts itself (X-MATE-AutoRestart).

/// Whose program draws the X11 desktop icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIconsOwner {
    /// MATE.
    Caja,
    /// Cinnamon.
    Nemo,
    /// XFCE.
    Xfdesktop,
}

impl DesktopIconsOwner {
    /// Program name for the person (program names are not translated).
    pub fn program_name(self) -> &'static str {
        match self {
            DesktopIconsOwner::Caja => "Caja",
            DesktopIconsOwner::Nemo => "Nemo",
            DesktopIconsOwner::Xfdesktop => "xfdesktop",
        }
    }
}

/// Who draws the icons on this desktop; `None` — not X11, or a desktop where
/// the wallpaper window does not get in the way of the icons (or we do not
/// know it).
///
/// `de` — the environment name from `wallpaper_engine::detect_de`,
/// `session_type` — from `wallpaper_engine::detect_session_type`.
pub fn desktop_icons_owner(de: &str, session_type: &str) -> Option<DesktopIconsOwner> {
    if session_type != "x11" {
        return None;
    }
    match de {
        "mate" => Some(DesktopIconsOwner::Caja),
        "cinnamon" | "cinnamon-mint" => Some(DesktopIconsOwner::Nemo),
        "xfce" => Some(DesktopIconsOwner::Xfdesktop),
        _ => None,
    }
}

pub const MATE_BACKGROUND_SCHEMA: &str = "org.mate.background";
pub const MATE_SHOW_ICONS_KEY: &str = "show-desktop-icons";
pub const CAJA_DESKTOP_SCHEMA: &str = "org.mate.caja.desktop";
pub const CAJA_TRANSPARENT_KEY: &str = "transparent-background";
pub const MATE_SESSION_COMPONENTS_SCHEMA: &str = "org.mate.session.required-components";
pub const MATE_WINDOW_MANAGER_KEY: &str = "windowmanager";
pub const MARCO_GENERAL_SCHEMA: &str = "org.mate.Marco.general";
pub const MARCO_COMPOSITING_KEY: &str = "compositing-manager";

/// The pause between "icons off" and "icons on" when recreating Caja's
/// desktop window: Caja merges both changes arriving at once into one and
/// does not recreate the window (measured on the test bench 31.10.2026).
pub const CAJA_DESKTOP_RECREATE_PAUSE_MS: u64 = 1500;

/// What icon preparation and rollback touch the machine with. The live
/// implementation is `wallpaper_exec::DesktopIconsSys`, fakes are in the checks.
pub trait DesktopIconsIo {
    /// The command output if it finished with code 0; otherwise `None`.
    fn run(&mut self, argv: &[&str]) -> Option<String>;
    /// The recorded previous desktop state or `None`.
    fn read_state(&mut self) -> Option<String>;
    /// Write the state (`None` — erase). True — it worked.
    fn write_state(&mut self, content: Option<&str>) -> bool;
    /// Wait.
    fn pause_ms(&mut self, ms: u64);
}

/// The previous desktop state file: `$XDG_STATE_HOME/rexon/desktop-icons.state`
/// (by default `~/.local/state/…`).
pub fn desktop_icons_state_path(xdg_state_home: &str, home: &str) -> String {
    let base = if xdg_state_home.trim().is_empty() {
        format!("{}/.local/state", home.trim_end_matches('/'))
    } else {
        xdg_state_home.trim_end_matches('/').to_string()
    };
    format!("{base}/rexon/desktop-icons.state")
}

fn gsettings_get(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> Option<String> {
    io.run(&["gsettings", "get", schema, key]).map(|out| out.trim().trim_matches('\'').to_string())
}

fn gsettings_bool(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> Option<bool> {
    match gsettings_get(io, schema, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn gsettings_set(io: &mut impl DesktopIconsIo, schema: &str, key: &str, value: bool) -> bool {
    let value = if value { "true" } else { "false" };
    let ok = io.run(&["gsettings", "set", schema, key, value]).is_some();
    if !ok {
        crate::log_warn!("desktop-icons", "не записалось {schema} {key} = {value}");
    }
    ok
}

/// Whether the installed Caja has the transparent desktop background key — by the schema.
fn caja_has_transparent_key(io: &mut impl DesktopIconsIo) -> bool {
    io.run(&["gsettings", "list-keys", CAJA_DESKTOP_SCHEMA])
        .is_some_and(|keys| keys.lines().any(|k| k.trim() == CAJA_TRANSPARENT_KEY))
}

/// The MATE window manager is Marco (its compositor is what we enable). If
/// not found out — Marco: it is the MATE default.
fn mate_wm_is_marco(io: &mut impl DesktopIconsIo) -> bool {
    gsettings_get(io, MATE_SESSION_COMPONENTS_SCHEMA, MATE_WINDOW_MANAGER_KEY)
        .is_none_or(|wm| wm.is_empty() || wm.eq_ignore_ascii_case("marco"))
}

/// Recreate Caja's desktop window without touching the file manager windows.
fn recreate_caja_desktop(io: &mut impl DesktopIconsIo) {
    gsettings_set(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY, false);
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    gsettings_set(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY, true);
}

/// An icons window that can be recreated by toggling "icons off → on": the
/// schema and key by the window's `WM_CLASS` (instance, class); `None` — we
/// cannot (xfdesktop draws the background opaque, recreating will not help).
///
/// Needed where the window manager does not put the wallpaper window under
/// the icons window on request: xfwm4 discards ConfigureWindow for "desktop"
/// type windows (`clientMoveResizeWindow`), and does not support
/// `_NET_RESTACK_WINDOW`. It puts a new icons window over the wallpaper
/// window — on the test bench (Xvfb, xfwm4 4.20 with the compositor,
/// 31.10.2026) that is how it goes for both nemo-desktop and Caja with a
/// transparent background.
pub fn icons_window_toggle(instance: &str, class: &str) -> Option<(&'static str, &'static str)> {
    let is = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    if is(instance, "nemo-desktop") || is(class, "nemo-desktop") {
        return Some((NEMO_DESKTOP_SCHEMA, NEMO_SHOW_ICONS_KEY));
    }
    if is(instance, "desktop_window") && is(class, "caja") {
        return Some((MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY));
    }
    None
}

/// Recreate the icons window by toggling `schema key` "off → on" with the
/// [`CAJA_DESKTOP_RECREATE_PAUSE_MS`] pause (without it Caja would merge both
/// changes; nemo-desktop recreates even without the pause, it does no harm there).
///
/// The key is off (no icons) or cannot be read — nothing is touched. If it did
/// not get turned back on — one more attempt: the icons must not be left
/// turned off. True — the window was recreated and the key is on again.
pub fn recreate_icons_window(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> bool {
    if gsettings_bool(io, schema, key) != Some(true) {
        crate::log_info!("desktop-icons", "{schema} {key} не включён — пересоздавать окно значков незачем");
        return false;
    }
    if !gsettings_set(io, schema, key, false) {
        return false;
    }
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    if gsettings_set(io, schema, key, true) {
        return true;
    }
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    let back = gsettings_set(io, schema, key, true);
    if !back {
        crate::log_warn!("desktop-icons", "{schema} {key} не вернулся в true — значки стола выключены");
    }
    back
}

/// The previous desktop state: what Rexon changed and what to restore it to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopIconsSaved {
    /// The Caja part is recorded (the `owner=caja` line); repeated applies do not
    /// overwrite it — they would see our own values.
    pub caja_recorded: bool,
    /// Caja's transparent background key was off — restore `false`.
    pub caja_transparent_was_off: bool,
    /// The Marco compositor was off — restore `false`.
    pub marco_compositing_was_off: bool,
    /// Rexon appended itself to `org.nemo.desktop ignored-desktop-handlers`;
    /// here is the previous value — GVariant text, as gsettings prints it.
    pub nemo_ignored_before: Option<String>,
}

impl DesktopIconsSaved {
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if self.caja_recorded {
            text.push_str(&format!(
                "owner=caja\ncaja-transparent-was-off={}\nmarco-compositing-was-off={}\n",
                self.caja_transparent_was_off, self.marco_compositing_was_off
            ));
        }
        if let Some(before) = &self.nemo_ignored_before {
            text.push_str(&format!("nemo-ignored-desktop-handlers-before={}\n", one_line(before)));
        }
        text
    }

    /// Parse what was recorded; a foreign or broken file — `None` (nothing to roll back).
    pub fn parse(text: &str) -> Option<Self> {
        let mut saved = DesktopIconsSaved::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let flag = value.trim() == "true";
            match key.trim() {
                "owner" => saved.caja_recorded = value.trim() == "caja",
                "caja-transparent-was-off" => saved.caja_transparent_was_off = flag,
                "marco-compositing-was-off" => saved.marco_compositing_was_off = flag,
                "nemo-ignored-desktop-handlers-before" if parse_gvariant_strv(value).is_some() => {
                    saved.nemo_ignored_before = Some(value.trim().to_string());
                }
                _ => {}
            }
        }
        if !saved.caja_recorded {
            saved.caja_transparent_was_off = false;
            saved.marco_compositing_was_off = false;
        }
        (saved.caja_recorded || saved.nemo_ignored_before.is_some()).then_some(saved)
    }
}

fn one_line(text: &str) -> String {
    text.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

/// The recorded previous state, supplemented with `change` — back into the
/// file. Parts recorded earlier are not lost: Caja and Nemo each write their own.
fn update_saved(io: &mut impl DesktopIconsIo, change: impl FnOnce(&mut DesktopIconsSaved)) {
    let mut saved = io.read_state().and_then(|s| DesktopIconsSaved::parse(&s)).unwrap_or_default();
    change(&mut saved);
    if !io.write_state(Some(&saved.to_text())) {
        crate::log_warn!("desktop-icons", "прежнее состояние стола не записалось — откатить его будет нечем");
    }
}

pub const NEMO_DESKTOP_SCHEMA: &str = "org.nemo.desktop";
pub const NEMO_IGNORED_HANDLERS_KEY: &str = "ignored-desktop-handlers";
/// nemo-desktop desktop icons: "off → on" recreates its window.
pub const NEMO_SHOW_ICONS_KEY: &str = "show-desktop-icons";
/// Our value in `ignored-desktop-handlers`: a `WM_CLASS` substring of all
/// wallpaper hosts (`RexonVideoWallpaper`, `RexonWebWallpaper`, …).
pub const NEMO_REXON_HANDLER: &str = "rexon";

fn is_rexon_handler(entry: &str) -> bool {
    entry.trim().eq_ignore_ascii_case(NEMO_REXON_HANDLER)
}

/// Parsing a GVariant string array as `gsettings get` prints it:
/// `['conky', 'csd-background']`, `@as []`, single or double quotes, `\` —
/// escaping. Not a string array — `None`.
pub fn parse_gvariant_strv(text: &str) -> Option<Vec<String>> {
    let t = text.trim();
    let t = t.strip_prefix("@as").map_or(t, str::trim_start);
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let quote = match chars.next() {
            None => return Some(out),
            Some(q @ ('\'' | '"')) => q,
            Some(_) => return None,
        };
        let mut item = String::new();
        loop {
            match chars.next()? {
                '\\' => item.push(chars.next()?),
                c if c == quote => break,
                c => item.push(c),
            }
        }
        out.push(item);
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        match chars.next() {
            None => return Some(out),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

/// A string array into GVariant text for `gsettings set`.
pub fn format_gvariant_strv(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = items
        .iter()
        .map(|s| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("[{}]", quoted.join(", "))
}

fn nemo_has_ignored_key(io: &mut impl DesktopIconsIo) -> bool {
    io.run(&["gsettings", "list-keys", NEMO_DESKTOP_SCHEMA])
        .is_some_and(|keys| keys.lines().any(|k| k.trim() == NEMO_IGNORED_HANDLERS_KEY))
}

fn nemo_ignored_get(io: &mut impl DesktopIconsIo) -> Option<(String, Vec<String>)> {
    let raw = io.run(&["gsettings", "get", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY])?;
    let raw = raw.trim().to_string();
    let list = parse_gvariant_strv(&raw)?;
    Some((raw, list))
}

fn nemo_ignored_set(io: &mut impl DesktopIconsIo, value: &str) -> bool {
    let ok = io.run(&["gsettings", "set", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY, value]).is_some();
    if !ok {
        crate::log_warn!("desktop-icons", "не записалось {NEMO_DESKTOP_SCHEMA} {NEMO_IGNORED_HANDLERS_KEY} = {value}");
    }
    ok
}

/// nemo-desktop must not take the wallpaper window for a foreign desktop:
/// append `rexon` to `ignored-desktop-handlers` where this schema exists.
/// Foreign values stay; the previous value is recorded on the first append.
fn prepare_nemo_handlers(io: &mut impl DesktopIconsIo) {
    if !nemo_has_ignored_key(io) {
        return;
    }
    let Some((raw, mut list)) = nemo_ignored_get(io) else {
        crate::log_warn!("desktop-icons", "{NEMO_DESKTOP_SCHEMA} {NEMO_IGNORED_HANDLERS_KEY} не прочитался — не трогаю");
        return;
    };
    if list.iter().any(|e| is_rexon_handler(e)) {
        return;
    }
    update_saved(io, |saved| {
        if saved.nemo_ignored_before.is_none() {
            saved.nemo_ignored_before = Some(raw.clone());
        }
    });
    list.push(NEMO_REXON_HANDLER.to_string());
    if nemo_ignored_set(io, &format_gvariant_strv(&list)) {
        crate::log_info!("desktop-icons", "nemo-desktop: окно обоев в {NEMO_IGNORED_HANDLERS_KEY} (было {raw})");
    }
}

/// Remove `rexon` from `ignored-desktop-handlers`. If nobody has changed the
/// list since then — restore the recorded value as it was; otherwise remove
/// only ours and keep the foreign edits.
fn restore_nemo_handlers(io: &mut impl DesktopIconsIo, before: &str) {
    let Some((_, current)) = nemo_ignored_get(io) else {
        return;
    };
    if !current.iter().any(|e| is_rexon_handler(e)) {
        return;
    }
    let rest: Vec<String> = current.into_iter().filter(|e| !is_rexon_handler(e)).collect();
    if parse_gvariant_strv(before).as_deref() != Some(rest.as_slice()) {
        nemo_ignored_set(io, &format_gvariant_strv(&rest));
        return;
    }
    // The previous value may have been the schema default — then a reset
    // restores it without leaving an entry in dconf that was not there.
    let _ = io.run(&["gsettings", "reset", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY]);
    if nemo_ignored_get(io).map(|(_, list)| list) != Some(rest) {
        nemo_ignored_set(io, before);
    }
}

/// The icons will be hidden: the desktop file manager cannot do a transparent background.
pub fn desktop_icons_hidden_text(owner: DesktopIconsOwner) -> String {
    crate::i18n::trf(
        "Desktop icons will be hidden while the live wallpaper is on: %1 cannot draw a \
         see-through desktop yet. The icons come back when the live wallpaper stops.",
        &[&owner.program_name()],
    )
}

/// The icons stay: Rexon turned on Caja's transparent desktop background
/// (and, if needed, the Marco compositor).
pub fn desktop_icons_kept_text(compositing_turned_on: bool) -> String {
    let mut text = crate::i18n::tr(
        "Desktop icons stay on top of the live wallpaper: Rexon turned on the see-through \
         desktop background in Caja.",
    );
    if compositing_turned_on {
        text.push(' ');
        text.push_str(&crate::i18n::tr("Marco's compositing was turned on for this as well."));
    }
    text
}

/// Prepare the desktop for a live wallpaper: what to tell the person (`None` — nothing).
///
/// Called on every live wallpaper apply as a window on X11; repeating is
/// harmless: what is already configured is not touched, the previous state is
/// recorded only by the first apply.
///
/// The nemo-desktop key is edited on any of these desktops where its schema
/// exists: nemo-desktop is also run outside Cinnamon (its fallback mode), and
/// its desktop window check is the same.
pub fn prepare_desktop_icons(io: &mut impl DesktopIconsIo, owner: DesktopIconsOwner) -> Option<String> {
    prepare_nemo_handlers(io);
    match owner {
        DesktopIconsOwner::Nemo => None,
        DesktopIconsOwner::Xfdesktop => {
            // 0 — no icons; unset — the xfdesktop default (files).
            let style = io.run(&["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/style"]);
            (style.as_deref().map(str::trim) != Some("0")).then(|| desktop_icons_hidden_text(owner))
        }
        DesktopIconsOwner::Caja => prepare_caja(io),
    }
}

fn prepare_caja(io: &mut impl DesktopIconsIo) -> Option<String> {
    // No icons (or no MATE schema at all) — nothing to cover.
    if gsettings_bool(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY) != Some(true) {
        return None;
    }
    if !caja_has_transparent_key(io) {
        crate::log_info!("desktop-icons", "у Caja нет ключа {CAJA_TRANSPARENT_KEY} — значки будут под живыми обоями");
        return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
    }
    let transparent_on = gsettings_bool(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY) == Some(true);
    let marco = mate_wm_is_marco(io);
    let compositing_off = marco && gsettings_bool(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY) == Some(false);
    if transparent_on && !compositing_off {
        return None;
    }
    // Record what was there — only on the first apply: a repeated one would see
    // our own values.
    if !io.read_state().and_then(|s| DesktopIconsSaved::parse(&s)).is_some_and(|s| s.caja_recorded) {
        update_saved(io, |saved| {
            saved.caja_recorded = true;
            saved.caja_transparent_was_off = !transparent_on;
            saved.marco_compositing_was_off = compositing_off;
        });
    }
    if compositing_off && !gsettings_set(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY, true) {
        return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
    }
    if !transparent_on {
        if !gsettings_set(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY, true) {
            return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
        }
        recreate_caja_desktop(io);
    }
    crate::log_info!(
        "desktop-icons",
        "Caja: сквозной фон стола включён{}",
        if compositing_off { ", композитор Marco включён" } else { "" }
    );
    Some(desktop_icons_kept_text(compositing_off))
}

/// Restore the desktop as it was before the first live wallpaper apply. True —
/// there was something to restore.
pub fn restore_desktop_icons(io: &mut impl DesktopIconsIo) -> bool {
    let Some(text) = io.read_state() else {
        return false;
    };
    let Some(saved) = DesktopIconsSaved::parse(&text) else {
        io.write_state(None);
        return false;
    };
    if saved.caja_transparent_was_off
        && gsettings_bool(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY) == Some(true)
        && gsettings_set(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY, false)
        && gsettings_bool(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY) == Some(true)
    {
        recreate_caja_desktop(io);
    }
    if saved.marco_compositing_was_off {
        gsettings_set(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY, false);
    }
    if let Some(before) = &saved.nemo_ignored_before {
        restore_nemo_handlers(io, before);
    }
    io.write_state(None);
    crate::log_info!("desktop-icons", "стол возвращён как был до живых обоев");
    true
}
