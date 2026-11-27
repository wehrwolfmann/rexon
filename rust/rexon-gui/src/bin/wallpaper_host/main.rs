// rexon-wallpaper-host — ONE Rexon wallpaper host without Python in the
// process.
//
// Video (QtMultimedia) and web (QtWebEngine) are modes of one binary, not
// two hosts: on 15.09.2026 the owner decided not to multiply hosts. The
// command line is built by the core
// (`rexon-core::wallpaper_engine::wallpaper_host_argv`), and the mode is
// set by the `--mode` option. Here is the shared Qt start-up; what to show
// is decided by `video.rs` and `web.rs`, and the mode options are parsed in
// src/wallpaper_host.rs and src/web_host.rs.
//
// Run:
//     rexon-wallpaper-host --mode video /path/video.mp4 [--no-audio]
//     rexon-wallpaper-host --mode video /wallpaper/dir             # with project.json
//     rexon-wallpaper-host --mode web https://example.com
//     rexon-wallpaper-host --mode web /path/index.html --windowed  # a 1280×720 window
//     rexon-wallpaper-host --mode swf /path/clip.swf
//     rexon-wallpaper-host /path/video.mp4     # without --mode — video (the previous contract)
//     rexon-wallpaper-host --mode waywallen [--socket <display.sock>]  # daemon output on X11
// Self-test (reports and closes by itself, the exit code is the verdict):
//     rexon-wallpaper-host --mode web /path/index.html --selftest
//
// Warning: laziness of modes. Chromium is brought up ONLY by web mode: the
// QtWebEngineQuick library is opened with `dlopen` from `web.rs` (analysis
// in src/web_host.rs), it is not in the binary's dependency table, and
// video wallpapers do not pay for it on every start. QtMultimedia is loaded
// by the video QML itself.
//
// The third mode is Flash (SWF, Ruffle through wgpu, the `rexon-swf`
// crate): it has no QML but its own render loop, so it does not enter
// `run_qml` and returns the code by itself (`swf.rs`). The exit codes come
// from the same core table (`wallpaper_engine::host_exit`). Vulkan and
// Ruffle are brought up ONLY by this mode: video and web do not pay for
// them at start.

mod swf;
mod video;
mod waywallen;
mod web;

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString, QUrl,
    QVariant,
};

use rexon_core::wallpaper_engine::{host_exit, HostMode};
use rexon_gui::wallpaper_host::{
    host_placement, install_host_log, install_panic_guard, keep_alive_without_windows, prepare_qpa_platform,
    take_mode, verdict, PLACEMENT_OFFSCREEN, USAGE,
};
// The link to the crate library is mandatory, not decorative: without it
// rustc does not pull in the rlib, and with it the cxx-qt static archives
// (moc, the QML plugin with resources) — the binary would build, but the
// QML would be empty.
use rexon_gui::wallpaper_audio::qobject::WallpaperAudio;
use rexon_gui::wallpaper_host::qobject::WallpaperPlacement;

fn main() {
    // Warning: as the FIRST line, before any Qt. A panic of the wallpaper host
    // has no right to unwind the stack: unwinding goes through Qt teardown,
    // and on X11 that waits for the event queue thread — the host once hung
    // forever on that wait (analysis in `wallpaper_host::verdict`). Wallpapers
    // of all modes are brought up again by the executor, while nobody would
    // bring up an eternally living empty process.
    install_panic_guard();
    std::process::exit(run());
}

/// Exit codes are the core's shared table `wallpaper_engine::host_exit`: 1 —
/// nothing to show, 2 — the QML did not come up, 3 — the frame stream did
/// not start; video also has 4 (audio requested, no track), web has 5 (the
/// page did not load) — both are set by the self-test from QML; Flash has 7
/// (no Vulkan GPU for wallpapers) and 8 (the clip did not load).
fn run() -> i32 {
    // The reference point of the "first frame on screen" measurement.
    rexon_gui::wallpaper_host::mark_host_started();
    // The Rexon interface language — before everything else: the name of the
    // wallpaper audio stream in the system mixer is taken from the translation
    // catalogue (`wallpaper_audio::stream_description`).
    rexon_core::i18n::init();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mode, rest) = match take_mode(&argv) {
        Ok(taken) => taken,
        Err(message) => return refuse(&message),
    };
    // The log — before parsing the mode: the `-START` lines and plan refusals
    // are already events. The help and invalid options above are command
    // output, not the log.
    install_host_log(rest.iter().any(|a| a == "--selftest" || a.starts_with("--selftest=")));
    let cinnamon_layer_shell = rexon_gui::wallpaper_host::cinnamon_layer::init_fact();
    // Showing frames into a file is ordered only by a shell extension. On
    // Cinnamon with layer-shell Rexon puts the wallpaper as a desktop layer,
    // and such a start is a second host next to ours (a copy of the extension
    // from an earlier version). Code 0: an extension of any version treats
    // this as "removed by ourselves" and stays silent.
    let stream = rexon_core::wallpaper_mmap::env_output_path(std::env::var("REXON_WP_OUTPUT").ok().as_deref()).is_some();
    if mode != HostMode::Waywallen && rexon_core::wallpaper_engine::stream_host_refused(stream, cinnamon_layer_shell) {
        rexon_core::log_warn!(
            "wallpaper-host",
            "расширение оболочки просит обои кадрами, а на Cinnamon с layer-shell их показывает Rexon слоем стола — второй хост не поднимаю"
        );
        return host_exit::OK;
    }
    // One display host per session: the previous one (if any) is replaced —
    // but only when there is definitely something to show (the plan came
    // together), otherwise an invalid start would remove a working wallpaper.
    let take_lock = || rexon_gui::wallpaper_host::session_lock::take(mode, &rest);
    let planned = match mode {
        HostMode::Video => video::plan(&rest),
        HostMode::Web => web::plan(&rest),
        // The waywallen daemon output client on X11 (Wallpaper Engine scenes).
        HostMode::Waywallen => waywallen::plan(&rest),
        // Flash is drawn by wgpu, not QML: its own loop, its own codes from the
        // same table. The Qt platform is set by this file — the same shared
        // decision as for the QML modes in `run_qml`.
        HostMode::Swf => {
            return match swf::plan(&rest) {
                Ok(plan) => {
                    take_lock();
                    prepare_qpa_platform(plan.placement, HostMode::Swf);
                    swf::run(plan)
                }
                Err(code) => code,
            };
        }
    };
    match planned {
        Ok(plan) => {
            take_lock();
            run_qml(plan)
        }
        Err(message) => refuse(&message),
    }
}

/// Refusal before any Qt: help is not an error; everything else means
/// nothing to show.
fn refuse(message: &str) -> i32 {
    eprintln!("{message}");
    if message == USAGE {
        host_exit::OK
    } else {
        host_exit::NOTHING_TO_SHOW
    }
}

/// Everything the mode's window needs, computed BEFORE the first Qt call.
pub struct QmlPlan {
    /// Module name in the core's entry point table (`rexon-core::qt_app`). The
    /// modes have no application name string of their own: without
    /// `desktopFileName` KDE on Wayland draws a default logo in the task bar
    /// instead of the icon.
    pub entry_module: &'static str,
    /// QML address inside the static resource of the `org.rexon.gui` module.
    pub qml_url: &'static str,
    /// Log line tag of the mode (`REXON-WPH` for video, `REXON-WEB` for web) —
    /// the former tags of the two hosts are kept: logs are searched by them.
    pub tag: &'static str,
    /// Host mode: it determines what captures the frame in stream mode and,
    /// therefore, which Qt platform to live on
    /// (`wallpaper_mmap::host_stream_capture`).
    pub mode: HostMode,
    pub placement: &'static str,
    pub properties: QMap<QMapPair_QString_QVariant>,
    pub selftest: bool,
    /// The mode's action BEFORE `QGuiApplication` (the audio stream role for
    /// video, bringing up Chromium for web). An error means "the QML did not
    /// come up".
    pub before_app: fn(&QmlPlan) -> Result<(), String>,
}

fn run_qml(plan: QmlPlan) -> i32 {
    // Warning: BEFORE QGuiApplication is created: the Qt platform is chosen by
    // the application constructor. Without this, on GNOME/Cinnamon Wayland the
    // wallpaper window in frame stream mode came out as an ordinary window on
    // top of the desktop. And BEFORE Chromium initialization: it reads the
    // platform when coming up.
    prepare_qpa_platform(plan.placement, plan.mode);

    // Without this a run from a script looks mute: when stderr is not a
    // terminal, Qt on Arch/CachyOS sends messages to journald, and the QML
    // self-test report does not reach the output.
    if plan.selftest {
        unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    }

    if let Err(message) = (plan.before_app)(&plan) {
        rexon_core::log_error!("wallpaper-host", "{}-ERROR {message}", plan.tag);
        return host_exit::QML_FAILED;
    }

    // MANDATORY: the static QML plugin with resources and type registration is
    // linked as a separate archive, and without an explicit reference the
    // linker drops it — QML then fails with "qrc:/… No such file or directory"
    // before any import error.
    cxx_qt::init_qml_module!("org.rexon.gui");

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        apply_qt_identity(app, plan.entry_module);
    }
    // Warning: RIGHT after the application and before the first window. The
    // host has as many windows as monitors; a disconnected monitor takes its
    // window away, and the Qt default "the last window was closed — quit"
    // would kill the whole wallpaper process for no reason (see
    // `wallpaper_host::keep_alive_without_windows`).
    keep_alive_without_windows();

    let tag = plan.tag;
    let mut engine = QQmlApplicationEngine::new();
    let Some(mut engine) = engine.as_mut() else {
        rexon_core::log_error!("wallpaper-host", "{tag}-ERROR движок QML не создался");
        return verdict(host_exit::QML_FAILED);
    };

    // The properties go to the QML root as initial properties: context
    // properties are not wrapped in cxx-qt-lib 0.9, while setInitialProperties
    // exists and is applied BEFORE the first binding — that is, before the
    // window decides what it should be.
    engine.as_mut().set_initial_properties(&plan.properties);

    // The verdict "the QML did not load" is given by a signal, not by silence:
    // without it an empty engine would exit instantly with code 0, and "no
    // wallpaper" would look like "all is well".
    // Arc/AtomicBool, not Rc/Cell: cxx-qt requires the signal handler to be
    // Send — the signal may come from a thread other than the main one.
    let loaded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = loaded.clone();
    let _guard = engine
        .as_mut()
        .on_object_created(move |_engine, object, url| {
            if object.is_null() {
                rexon_core::log_error!("wallpaper-host", "{tag}-ERROR QML не загрузился: {}", url.to_string());
            } else {
                seen.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });

    engine.as_mut().load(&QUrl::from(plan.qml_url));
    if !loaded.load(std::sync::atomic::Ordering::SeqCst) {
        return verdict(host_exit::QML_FAILED);
    }

    // The verdict goes through `verdict`, not as a bare number: right after
    // this line Qt teardown begins, and it must fit into a deadline (analysis
    // there).
    let code = verdict(match app.as_mut() {
        Some(app) => app.exec(),
        None => host_exit::QML_FAILED,
    });
    // The player — stop and an empty source BEFORE tearing down the engine and
    // the windows, already under the `verdict` deadline. Otherwise the window
    // is destroyed while the decoding thread is still working — the same class
    // of crash as the main window has at exit (SIGSEGV in `av_log`, analysis
    // in `bin/main_window.rs` at the same call). Exit here happens on `Qt.quit()`
    // of the debug window.
    unsafe { rexon_stop_media_players() };
    code
}

// The shim is `cpp/rexon_gui_factory.cpp`, shared with the main window.
extern "C" {
    fn rexon_stop_media_players() -> std::ffi::c_int;
}

/// How the application presents itself to the system — from the shared
/// entry point table.
fn apply_qt_identity(app: std::pin::Pin<&mut QGuiApplication>, entry_module: &str) {
    let Some(identity) = rexon_core::qt_app::identity(entry_module) else {
        return;
    };
    let mut app = app;
    app.as_mut()
        .set_application_name(&QString::from(identity.application_name));
    if let Some(name) = identity.display_name {
        app.as_mut().set_application_display_name(&QString::from(name));
    }
    if let Some(name) = identity.organization_name {
        app.as_mut().set_organization_name(&QString::from(name));
    }
    if let Some(name) = identity.desktop_file_name {
        QGuiApplication::set_desktop_file_name(&QString::from(name));
    }
}

/// The display environment — ONE for all modes: where to write frames,
/// their size, rate and how to place the window. All decisions are the
/// core's, there are none of our own here. Previously each host read it
/// with its own copy.
pub struct HostEnv {
    pub mmap_output: Option<String>,
    pub mmap_w: i64,
    pub mmap_h: i64,
    pub fps: i64,
    pub placement: &'static str,
}

impl HostEnv {
    pub fn read(windowed: bool) -> HostEnv {
        let env = |name: &str| std::env::var(name).ok();
        let mmap_output =
            rexon_core::wallpaper_mmap::env_output_path(env("REXON_WP_OUTPUT").as_deref());
        let (mmap_w, mmap_h) = rexon_core::wallpaper::mmap_size(
            env("REXON_WP_WIDTH").as_deref(),
            env("REXON_WP_HEIGHT").as_deref(),
        );
        let fps = rexon_core::wallpaper_mmap::env_fps(env("REXON_WP_FPS").as_deref());
        let wayland = rexon_core::wallpaper_mmap::is_wayland_session(
            &env("XDG_SESSION_TYPE").unwrap_or_default(),
            &env("WAYLAND_DISPLAY").unwrap_or_default(),
        );
        // The display method must be the same for all modes, otherwise on one
        // desktop video would go into a layer while web went into a window on top
        // of everything.
        let placement = host_placement(mmap_output.is_some(), windowed, wayland);
        HostEnv {
            mmap_output,
            mmap_w,
            mmap_h,
            fps,
            placement,
        }
    }

    /// QML root properties shared by all modes, and honest refusals before any
    /// Qt.
    pub fn common_properties(
        &self,
        properties: &mut QMap<QMapPair_QString_QVariant>,
        selftest_ms: i64,
    ) -> Result<(), String> {
        properties.insert(QString::from("placement"), variant_str(self.placement));
        properties.insert(
            QString::from("mmapPath"),
            variant_str(self.mmap_output.as_deref().unwrap_or("")),
        );
        properties.insert(QString::from("mmapFps"), QVariant::from(&(self.fps as i32)));
        properties.insert(
            QString::from("selftestMs"),
            QVariant::from(&(selftest_ms as i32)),
        );
        // Offscreen without a frame file is a silently black screen for the user,
        // so the refusal is honest and comes before any Qt.
        if self.placement == PLACEMENT_OFFSCREEN && self.mmap_output.is_none() {
            return Err("офскрин-режим без REXON_WP_OUTPUT: кадры некуда писать".to_string());
        }
        Ok(())
    }
}

pub fn variant_str(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

// The types are mentioned for linking the cxx-qt static archives: without
// a reference to them the crate's QML plugin is dropped by the linker, and
// `WallpaperPlacement` does not exist in QML.
#[allow(dead_code)]
fn keep_qml_types_linked() -> (&'static str, &'static str) {
    (
        std::any::type_name::<WallpaperPlacement>(),
        std::any::type_name::<WallpaperAudio>(),
    )
}
