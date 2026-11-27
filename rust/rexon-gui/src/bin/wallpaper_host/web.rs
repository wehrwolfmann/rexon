// "Web" mode of the wallpaper host: what to show and with which
// properties.
//
// Replacement for the removed `python -m rexon.web_wallpaper`: the same
// environment contract (REXON_WP_OUTPUT / WIDTH / HEIGHT / FPS), the same
// mmap file for the shell extension, the same three ways to put the window
// into the background. The design and its boundaries are analysed in
// src/web_host.rs.

use std::path::{Path, PathBuf};

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString};

use rexon_gui::wallpaper_host::{qml_value_to_variant, PLACEMENT_OFFSCREEN};
use rexon_gui::web_host::{init_web_engine, parse_args};

use crate::{variant_str, HostEnv, QmlPlan};

/// Module name in the core's entry point table (`rexon-core::qt_app`).
const ENTRY_MODULE: &str = "rexon.web_wallpaper";

/// QML address inside the static resource of the `org.rexon.gui` module.
const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperWeb.qml";

/// Log line tag of the mode.
const TAG: &str = "REXON-WEB";

pub fn plan(argv: &[String]) -> Result<QmlPlan, String> {
    let cli = parse_args(argv)?;
    // A link goes as is: it cannot be canonicalized, and there is nowhere to
    // take a `project.json` next to it from.
    let (target, user_props) = if rexon_core::wallpaper::is_web_url(&cli.target) {
        (cli.target.clone(), serde_json::Map::new())
    } else {
        from_local(&cli.target)?
    };

    let env = HostEnv::read(cli.windowed);
    let mut properties = QMap::<QMapPair_QString_QVariant>::default();
    for prop in
        rexon_core::wallpaper::web_context_properties(&target, cli.windowed, env.mmap_w, env.mmap_h)
    {
        properties.insert(QString::from(&prop.name), qml_value_to_variant(&prop.value));
    }
    env.common_properties(&mut properties, cli.selftest_ms)?;
    properties.insert(
        QString::from("previewMode"),
        qml_value_to_variant(&rexon_core::wallpaper::QmlValue::Bool(cli.preview)),
    );

    // JS injected into the page: Wallpaper Engine stubs and pause/volume
    // wrappers of the shared wallpaper audio model (before the page's first
    // script), passing the wallpaper properties with the audio tick that
    // follows (after the document loads). All texts come from the core.
    properties.insert(
        QString::from("polyfillJs"),
        variant_str(&format!(
            "{}\n{}",
            rexon_core::wallpaper::WE_POLYFILL_JS,
            rexon_core::wallpaper::WEB_PAUSE_JS
        )),
    );
    let after_load = format!(
        "{}\n{}",
        rexon_core::wallpaper::web_apply_defaults_js(&rexon_core::wallpaper::web_defaults_json(
            &user_props
        )),
        rexon_core::wallpaper::WE_TICKER_JS
    );
    properties.insert(QString::from("afterLoadJs"), variant_str(&after_load));

    if cli.verbose {
        rexon_core::log_info!(
            "wallpaper-host",
            "{TAG}-START адрес={target} способ={} окно={}",
            env.placement, cli.windowed
        );
    }

    Ok(QmlPlan {
        entry_module: ENTRY_MODULE,
        qml_url: QML_URL,
        tag: TAG,
        mode: rexon_core::wallpaper_engine::HostMode::Web,
        placement: env.placement,
        properties,
        selftest: cli.selftest_ms > 0,
        before_app,
    })
}

fn before_app(plan: &QmlPlan) -> Result<(), String> {
    // Warning: offscreen mode is configured BEFORE Chromium initializes: the
    // variable is read when the subsystem comes up, and changing it afterwards
    // does nothing. The Qt platform has already been set by the shared
    // `run_qml` at this point.
    if plan.placement == PLACEMENT_OFFSCREEN
        && std::env::var_os("QTWEBENGINE_CHROMIUM_FLAGS").is_none()
    {
        // Warning: exactly `--in-process-gpu` and NO `--disable-gpu`. With the GPU
        // disabled Chromium does not composite the canvas (canvas2D/WebGL) into
        // the Qt scene graph: a frame captured from the window would contain only
        // the DOM. The flag string is held by the core.
        // SAFETY: called from main before the Qt application is created and before
        // any threads — there are no environment readers yet.
        unsafe {
            std::env::set_var(
                "QTWEBENGINE_CHROMIUM_FLAGS",
                rexon_core::wallpaper::WEB_OFFSCREEN_CHROMIUM_FLAGS,
            )
        };
    }
    // Warning: as the FIRST Qt action and BEFORE QGuiApplication: Chromium
    // initialization sets up a shared OpenGL context for all windows of the
    // process, and after the application is created it is too late — the
    // WebEngineView would come up empty. And ONLY in this mode: the library is
    // opened here, video does not load it.
    init_web_engine()
}

/// A local address: the HTML itself, a wallpaper directory with
/// `project.json`, or a path to a file INSIDE such a directory.
fn from_local(raw: &str) -> Result<(String, serde_json::Map<String, serde_json::Value>), String> {
    let path = PathBuf::from(raw);
    let path = path.canonicalize().unwrap_or(path);

    if path.is_dir() {
        return from_wallpaper_dir(&path);
    }
    if !path.is_file() {
        // Exactly the same text the wallpaper engine answers the customer with.
        return Err(rexon_core::wallpaper_engine::web_missing_message(raw));
    }

    // A path to a file INSIDE a wallpaper directory: a `project.json` with
    // properties may lie next to it, and they must not be lost because the
    // user pointed at index.html rather than at the directory.
    let props = path
        .parent()
        .filter(|dir| dir.join("project.json").is_file())
        .map(project_props)
        .unwrap_or_default();
    Ok((path.to_string_lossy().into_owned(), props))
}

/// A wallpaper directory with `project.json`: parsing, type detection and
/// an honest refusal for everything this mode does not draw.
fn from_wallpaper_dir(
    dir: &Path,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), String> {
    let project_json = dir.join("project.json");
    if !project_json.is_file() {
        return Err(format!("в каталоге нет project.json: {}", dir.display()));
    }
    let (project, warnings) = rexon_core::user_props::parse_project_json(&project_json)
        .map_err(|e| format!("project.json не разобрался: {e}"))?;
    for w in &warnings {
        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN {w}");
    }

    // The `type` field may be missing altogether — both in some official WE
    // wallpapers and in ordinary Workshop items. Then the type is determined
    // by the directory content, as Wallpaper Engine itself does.
    let mut wp_type = project.r#type.clone();
    if wp_type.is_empty() {
        wp_type = rexon_core::user_props::infer_type_fs(dir, &project).to_string();
        rexon_core::log_warn!(
            "wallpaper-host",
            "{TAG}-WARN project.json без поля type — определено по содержимому: {}",
            if wp_type.is_empty() {
                "не определилось"
            } else {
                &wp_type
            }
        );
    }
    if let Some(reason) = rexon_core::wallpaper::web_host_reject_reason(
        &wp_type,
        &project_json.to_string_lossy(),
    ) {
        return Err(reason);
    }

    let entry = if project.file.is_empty() {
        dir.join(rexon_core::wallpaper::WEB_ENTRY_FILE)
    } else {
        dir.join(&project.file)
    };
    if !entry.is_file() {
        return Err(rexon_core::wallpaper_engine::web_missing_message(
            &entry.to_string_lossy(),
        ));
    }
    Ok((
        entry.to_string_lossy().into_owned(),
        project.defaults_for_js(),
    ))
}

/// Properties from the neighbouring `project.json`. A failed parse is no
/// reason to be left without a wallpaper: the properties go empty, and the
/// page takes its own.
fn project_props(dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    match rexon_core::user_props::parse_project_json(&dir.join("project.json")) {
        Ok((project, warnings)) => {
            for w in &warnings {
                rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN {w}");
            }
            project.defaults_for_js()
        }
        Err(e) => {
            rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN project.json не разобрался ({e}) — свойства не переданы");
            serde_json::Map::new()
        }
    }
}
