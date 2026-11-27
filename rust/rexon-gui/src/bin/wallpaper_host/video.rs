// "Video" mode of the wallpaper host: what to show and with which
// properties.
//
// Replacement for `python -m rexon.video_wallpaper`: the same arguments,
// the same environment variables, the same mmap file contract with the
// shell extensions. The design and its boundaries are analysed in
// src/wallpaper_host.rs.

use std::path::{Path, PathBuf};

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_gui::wallpaper_host::{
    apply_cli_overrides, parse_args, qml_value_to_variant as to_variant, Cli,
};

use crate::{HostEnv, QmlPlan};

/// Module name in the core's entry point table (`rexon-core::qt_app`).
const ENTRY_MODULE: &str = "rexon.video_wallpaper";

/// QML address inside the static resource of the `org.rexon.gui` module.
const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperVideo.qml";

/// Log line tag of the mode.
const TAG: &str = "REXON-WPH";

pub fn plan(argv: &[String]) -> Result<QmlPlan, String> {
    let cli = parse_args(argv)?;
    let (target_file, mut user_props) = resolve_target(&cli)?;
    apply_cli_overrides(&mut user_props, &cli);

    let env = HostEnv::read(cli.windowed);
    let target = target_file.to_string_lossy().into_owned();
    let mut properties = QMap::<QMapPair_QString_QVariant>::default();
    for prop in rexon_core::wallpaper::video_context_properties(
        &target,
        cli.windowed,
        &user_props,
        env.mmap_w,
        env.mmap_h,
    ) {
        properties.insert(QString::from(&prop.name), to_variant(&prop.value));
    }
    env.common_properties(&mut properties, cli.selftest_ms)?;
    // A volume from the command line cancels following the setting: a manual
    // run must sound exactly as requested. The option never comes from the
    // product — there the volume lives in the settings and changes on the fly.
    properties.insert(
        QString::from("volumeFixed"),
        QVariant::from(&cli.volume.unwrap_or(-1.0)),
    );

    if cli.verbose {
        rexon_core::log_info!(
            "wallpaper-host",
            "{TAG}-START файл={target} способ={} окно={} звук={:?} громкость={:?}",
            env.placement, cli.windowed, cli.audio, cli.volume
        );
    }

    Ok(QmlPlan {
        entry_module: ENTRY_MODULE,
        qml_url: QML_URL,
        tag: TAG,
        mode: rexon_core::wallpaper_engine::HostMode::Video,
        placement: env.placement,
        properties,
        selftest: cli.selftest_ms > 0,
        before_app,
    })
}

/// Warning: BEFORE the first Qt audio operation and before QGuiApplication
/// is created. Qt Multimedia does not expose the stream role: the value is
/// hard-coded into the library itself, and wallpapers went into the sound
/// system as `media.role=music` — that is, into the shared volume restore
/// slot with all music players and under ducking by other programs. The
/// role can only be set from outside, with an environment variable that
/// the sound client reads when it creates its context; the property list
/// is computed by the core.
fn before_app(_plan: &QmlPlan) -> Result<(), String> {
    for (name, value) in rexon_core::wallpaper_audio::stream_env() {
        if std::env::var_os(name).is_none() {
            // SAFETY: called from main before the Qt application is created and
            // before any threads — there are no environment readers yet.
            unsafe { std::env::set_var(name, &value) };
        }
    }
    Ok(())
}

/// What to play: a video file directly, or a file from a wallpaper
/// directory with the properties from its `project.json`.
fn resolve_target(
    cli: &Cli,
) -> Result<(PathBuf, serde_json::Map<String, serde_json::Value>), String> {
    let raw = PathBuf::from(&cli.path);
    let mut path = raw.canonicalize().unwrap_or(raw);

    // A path to a file INSIDE a wallpaper directory (`project.json`,
    // `index.html`…) — that is how the former shared wallpaper entry point was
    // called, and such calls must keep working: to someone who pointed at
    // project.json, the refusal "neither a video nor a directory" explains
    // nothing (rule #13).
    if rexon_core::wallpaper::use_parent_dir(&path.to_string_lossy(), path.is_file())
        && !is_video_file(&path)
    {
        if let Some(parent) = path.parent() {
            rexon_core::log_info!(
                "wallpaper-host",
                "{TAG} путь указывает на файл внутри каталога обоев — беру {}",
                parent.display()
            );
            path = parent.to_path_buf();
        }
    }

    let (target_file, user_props) = if is_video_file(&path) {
        // A direct path to a video file: it has no properties of its own.
        (path.clone(), serde_json::Map::new())
    } else if path.is_dir() {
        from_wallpaper_dir(&path)?
    } else {
        return Err(format!(
            "путь не видеофайл и не каталог обоев: {}",
            path.display()
        ));
    };

    if !target_file.is_file() {
        return Err(format!("видео не найдено: {}", target_file.display()));
    }
    Ok((target_file, user_props))
}

/// A wallpaper directory with `project.json`: parsing, type detection and
/// an honest refusal for everything this mode does not draw.
fn from_wallpaper_dir(
    dir: &Path,
) -> Result<(PathBuf, serde_json::Map<String, serde_json::Value>), String> {
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
    if let Some(reason) = rexon_core::wallpaper::video_host_reject_reason(
        &wp_type,
        &project_json.to_string_lossy(),
    ) {
        return Err(reason);
    }

    let target = if project.file.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(&project.file)
    };
    Ok((target, project.defaults_for_js()))
}

/// Whether this is a video file. The list of extensions is ONE for the
/// whole product and lives in the core.
fn is_video_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let lower = path.to_string_lossy().to_lowercase();
    rexon_core::user_props::VIDEO_ENTRY_EXTS
        .iter()
        .any(|ext| lower.ends_with(ext))
}
