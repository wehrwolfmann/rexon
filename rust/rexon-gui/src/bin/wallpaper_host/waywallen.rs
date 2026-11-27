// "waywallen output client" mode of the wallpaper host
// (`--mode waywallen`).
//
// Wallpaper Engine scenes are drawn by the waywallen daemon; on X11 outside
// KDE it has no output client of its own, and the host becomes that client:
// desktop windows with a waywallen output element (analysed in
// qml/WallpaperWaywallen.qml and `rexon-core::wallpaper_backend`, the
// "display for the daemon" section).
//
// It is started by two parties with a command line of the same form
// (`--socket <path> --mode waywallen`): the daemon — by its output
// detection at start-up, Rexon — on "Apply" when there are no displays.
// So two things happen here before Qt:
//   * on Wayland and without `DISPLAY` the host only waits — X11 windows
//     would lie on top of the desktop there, and the daemon would treat
//     the exit as a crash and restart it in a loop;
//   * one instance per session: a second one waits for the lock and takes
//     over the output if the first one dies. There are never two windows
//     per monitor.

use std::path::Path;

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use rexon_core::wallpaper_backend as wb;
use rexon_core::waywallen_kde_plugin as wkp;
use rexon_gui::wallpaper_host::{PLACEMENT_X11, USAGE};

use crate::{variant_str, QmlPlan};

/// Module name in the core's entry point table (`rexon-core::qt_app`): the
/// windows of this mode show scenes.
const ENTRY_MODULE: &str = "rexon.scene_wallpaper";

const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperWaywallen.qml";

/// Log line tag of the mode.
const TAG: &str = "REXON-WWD";

struct Args {
    socket: String,
    windowed: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut args = Args { socket: String::new(), windowed: false };
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--socket" => args.socket = it.next().ok_or("после --socket нужен путь к сокету вывода")?.clone(),
            other if other.starts_with("--socket=") => args.socket = other["--socket=".len()..].to_string(),
            "-w" | "--windowed" => args.windowed = true,
            "-v" | "--verbose" => {}
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("неизвестный ключ режима waywallen: {other}")),
        }
    }
    Ok(args)
}

/// The output element (the core's choice, `waywallen_kde_plugin::pick_display_item`):
/// the `Waywallen.Display` module of the `waywallen-display` distribution package
/// first, then the wrapper of the `org.waywallen.kde` plasmoid — the one bundled
/// into Rexon, the installed Plasma plugin as the last fallback.
fn find_display_item() -> Option<String> {
    let is_file = |p: &str| Path::new(p).is_file();
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let data_dir = rexon_core::installer::pick_package_data_dir(&rexon_core::cli::repo_root(), &is_file);
    let home = env("HOME");
    let plasmoids = wkp::plasmoid_display_wrappers(data_dir.as_deref(), &home);
    let import_dirs = wkp::qml_import_dirs(&env("QML_IMPORT_PATH"), &env("QML2_IMPORT_PATH"));
    let wrapper = wkp::system_display_wrapper_path(&env("XDG_RUNTIME_DIR"), &home);
    let choice = wkp::pick_display_item(&import_dirs, wrapper.as_deref(), &plasmoids, &is_file)?;
    if let wkp::DisplayItemSource::SystemModule { wrapper } = &choice {
        let written = Path::new(wrapper)
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| rexon_core::utils::atomic_write(wrapper, wkp::SYSTEM_DISPLAY_WRAPPER_QML).map_err(|e| e.to_string()));
        if let Err(e) = written {
            // The packaged module is there, but the wrapper could not be written —
            // fall back to a plasmoid rather than lose the output.
            rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN wrapper {wrapper} not written: {e}");
            return plasmoids.into_iter().find(|qml| is_file(qml));
        }
        rexon_core::log_info!("wallpaper-host", "{TAG} output element: packaged Waywallen.Display module");
    }
    Some(choice.qml().to_string())
}

/// Wait forever, showing nothing (Wayland, no `DISPLAY`, another instance
/// already shows the output). The process leaves on the daemon's signal.
fn idle_forever() -> ! {
    loop {
        std::thread::park();
    }
}

pub fn plan(argv: &[String]) -> Result<QmlPlan, String> {
    let args = parse(argv)?;
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    if !args.windowed && wb::rexon_host_should_idle(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"), &env("DISPLAY")) {
        if wb::is_wayland_session(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY")) {
            if let wb::WaylandHostRole::Delegate(client) = wayland_role(&args.socket) {
                supervise_layer_shell_client(client);
            }
        }
        rexon_core::log_info!(
            "wallpaper-host",
            "{TAG} сеанс не X11 (XDG_SESSION_TYPE={}, DISPLAY={}) — вывод здесь не мой, жду",
            env("XDG_SESSION_TYPE"),
            env("DISPLAY")
        );
        idle_forever();
    }

    if !args.windowed {
        take_display_lock();
    }

    let Some(item) = find_display_item() else {
        return Err(format!(
            "no waywallen output element: neither the {} package (Waywallen.Display module) nor the \
             org.waywallen.kde plasmoid in Rexon's data — nothing to show the output with",
            wkp::DISPLAY_MODULE_PACKAGE
        ));
    };
    plan_display(&args, item)
}

/// One instance per session: the lock is held until the end of the process.
/// If it is taken — wait until the previous instance leaves, and take over
/// the output.
fn take_display_lock() {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    if !runtime.is_empty() {
        let path = wb::rexon_host_lock_path(&runtime);
        match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) {
            Ok(file) => {
                if file.try_lock().is_err() {
                    rexon_core::log_info!("wallpaper-host", "{TAG} вывод уже показывает другой экземпляр — жду его ухода");
                    if let Err(e) = file.lock() {
                        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN блокировка {path}: {e}");
                    }
                    rexon_core::log_info!("wallpaper-host", "{TAG} прежний экземпляр ушёл — вывод мой");
                }
                std::mem::forget(file);
            }
            Err(e) => rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN блокировка {path} не открылась: {e}"),
        }
    }
}

/// Plan of an X11 window with the output element `item`.
fn plan_display(args: &Args, item: String) -> Result<QmlPlan, String> {
    let mut properties = QMap::<QMapPair_QString_QVariant>::default();
    properties.insert(
        QString::from("displayItemUrl"),
        QVariant::from(&QUrl::from_local_file(&QString::from(item.as_str()))),
    );
    properties.insert(QString::from("socketPath"), variant_str(&args.socket));
    properties.insert(QString::from("isWindowed"), QVariant::from(&args.windowed));
    rexon_core::log_info!(
        "wallpaper-host",
        "{TAG}-START элемент={item} сокет={}",
        if args.socket.is_empty() { "сам найдёт" } else { args.socket.as_str() }
    );
    Ok(QmlPlan {
        entry_module: ENTRY_MODULE,
        qml_url: QML_URL,
        tag: TAG,
        mode: rexon_core::wallpaper_engine::HostMode::Waywallen,
        placement: PLACEMENT_X11,
        properties,
        selftest: false,
        before_app,
    })
}

/// What the host does on Wayland: the compositor globals and the stock
/// output client (next to the daemon that started us, next to the
/// package's daemon, in `PATH`).
fn wayland_role(socket: &str) -> wb::WaylandHostRole {
    let globals = rexon_gui::screensaver_bridge::wayland_idle::probe().map(|f| f.globals).unwrap_or_default();
    let parent = std::fs::read_link(format!("/proc/{}/exe", std::os::unix::process::parent_id()))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let bin = wb::layer_shell_client_candidates(parent.as_deref(), &[])
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .or_else(|| which(wb::LAYER_SHELL_CLIENT_BIN));
    wb::wayland_host_role(&globals, bin.as_deref(), socket)
}

/// An executable from `PATH`.
fn which(name: &str) -> Option<String> {
    std::env::var("PATH")
        .ok()?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| format!("{}/{name}", d.trim_end_matches('/')))
        .find(|p| Path::new(p).is_file())
}

/// Bring up and watch the stock output client while the host itself is
/// alive. The "output released" marker
/// (`wallpaper_backend::display_off_marker_path`: the user set their own
/// wallpaper) — the client is stopped and not brought up until Rexon
/// removes it (applying a scene). The client dies together with the host
/// (`PR_SET_PDEATHSIG`): when the daemon stops the host, it also stops the
/// client.
fn supervise_layer_shell_client(client: Vec<String>) -> ! {
    use std::time::{Duration, Instant};
    take_display_lock();
    rexon_core::log_info!("wallpaper-host", "{TAG}-LAYER композитор умеет layer-shell — вывод сцен: {}", client.join(" "));
    let marker = wb::display_off_marker_live();
    let mut child: Option<(std::process::Child, Instant)> = None;
    let mut crashes: u32 = 0;
    let mut next_start = Instant::now();
    let mut released = false;
    loop {
        if Path::new(&marker).exists() {
            if let Some((mut c, _)) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            if !released {
                rexon_core::log_info!("wallpaper-host", "{TAG}-LAYER вывод сцен отпущен ради своих обоев");
                released = true;
            }
            crashes = 0;
            next_start = Instant::now();
        } else {
            released = false;
            if let Some((c, started)) = child.as_mut() {
                if let Ok(Some(status)) = c.try_wait() {
                    if started.elapsed() > Duration::from_secs(60) {
                        crashes = 0;
                    }
                    let delay = wb::layer_shell_restart_delay_ms(crashes);
                    crashes = crashes.saturating_add(1);
                    rexon_core::log_warn!(
                        "wallpaper-host",
                        "{TAG}-LAYER клиент вывода завершился ({status}) — подниму через {} с",
                        delay / 1000
                    );
                    next_start = Instant::now() + Duration::from_millis(delay);
                    child = None;
                }
            } else if Instant::now() >= next_start {
                match spawn_tied_to_us(&client) {
                    Ok(c) => {
                        rexon_core::log_info!("wallpaper-host", "{TAG}-LAYER клиент вывода поднят: pid {}", c.id());
                        child = Some((c, Instant::now()));
                        // The client surface is newer than the Cinnamon background service and
                        // went under it — put the service under us.
                        rexon_gui::wallpaper_host::cinnamon_layer::surface_up("вывод сцен Wallpaper Engine");
                    }
                    Err(e) => {
                        let delay = wb::layer_shell_restart_delay_ms(crashes);
                        crashes = crashes.saturating_add(1);
                        rexon_core::log_warn!("wallpaper-host", "{TAG}-LAYER клиент вывода не запустился: {e}");
                        next_start = Instant::now() + Duration::from_millis(delay);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Start a process that will get SIGTERM when the host dies.
fn spawn_tied_to_us(argv: &[String]) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(std::process::Stdio::null());
    // SAFETY: in the child process before exec — only prctl
    // (async-signal-safe).
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd.spawn()
}

/// The output element needs EGL: the daemon's frames arrive as DMA-BUF
/// buffers, and importing them into Qt on X11 works only through EGL (the
/// waywallen-display README about plasmashell on X11). By default Qt on
/// xcb picks GLX. The Qt platform is not touched here — it is set by the
/// shared `prepare_qpa_platform` in main.rs.
fn before_app(_plan: &QmlPlan) -> Result<(), String> {
    const GL_INTEGRATION: &str = "QT_XCB_GL_INTEGRATION";
    if std::env::var_os(GL_INTEGRATION).is_none_or(|v| v.is_empty()) {
        // SAFETY: called from main before the Qt application is created and before
        // any threads — there are no environment readers yet.
        unsafe { std::env::set_var(GL_INTEGRATION, "xcb_egl") };
    }
    Ok(())
}
