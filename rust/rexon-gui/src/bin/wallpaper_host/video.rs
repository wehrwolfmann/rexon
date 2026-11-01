// Режим «видео» хоста обоев: что показывать и с какими свойствами.
//
// Замена `python -m rexon.video_wallpaper`: те же аргументы, те же
// переменные окружения, тот же контракт mmap-файла с расширениями оболочки.
// Разбор устройства и границ — src/wallpaper_host.rs.

use std::path::{Path, PathBuf};

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_gui::wallpaper_host::{
    apply_cli_overrides, parse_args, qml_value_to_variant as to_variant, Cli,
};

use crate::{HostEnv, QmlPlan};

/// Имя модуля в таблице точек входа ядра (`rexon-core::qt_app`).
const ENTRY_MODULE: &str = "rexon.video_wallpaper";

/// Адрес QML внутри статического ресурса модуля `org.rexon.gui`.
const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperVideo.qml";

/// Метка строк журнала режима.
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
    // Громкость из командной строки отменяет слежение за настройкой:
    // ручной прогон обязан звучать ровно так, как попросили. Из продукта
    // ключ не приходит — там громкость живёт в настройках и меняется на
    // ходу.
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

/// ⚠️ ДО первой звуковой операции Qt и до создания QGuiApplication.
/// Роль потока Qt Multimedia наружу не отдаёт: значение зашито в саму
/// библиотеку, и обои уезжали в звуковую систему как `media.role=music` —
/// то есть в общую ячейку восстановления громкости со всеми музыкальными
/// плеерами и под приглушение чужими программами. Задать роль можно только
/// снаружи, переменной окружения, которую звуковой клиент читает при
/// создании своего контекста; список свойств считает ядро.
fn before_app(_plan: &QmlPlan) -> Result<(), String> {
    for (name, value) in rexon_core::wallpaper_audio::stream_env() {
        if std::env::var_os(name).is_none() {
            // SAFETY: зовётся из main до создания приложения Qt и до любых
            // потоков — читателей окружения ещё нет.
            unsafe { std::env::set_var(name, &value) };
        }
    }
    Ok(())
}

/// Что играть: видеофайл напрямую либо файл из каталога обоев со свойствами
/// из его `project.json`.
fn resolve_target(
    cli: &Cli,
) -> Result<(PathBuf, serde_json::Map<String, serde_json::Value>), String> {
    let raw = PathBuf::from(&cli.path);
    let mut path = raw.canonicalize().unwrap_or(raw);

    // Путь на файл ВНУТРИ каталога обоев (`project.json`, `index.html`…) —
    // так звали прежнюю общую точку входа обоев, и такие вызовы обязаны
    // продолжать работать: человеку, ткнувшему в project.json, отказ «не
    // видео и не каталог» ничего не объясняет (правило #13).
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
        // Прямой путь к видеофайлу: своих свойств у него нет.
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

/// Каталог обоев с `project.json`: разбор, определение типа и честный отказ
/// для всего, что рисует не этот режим.
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

    // Поля `type` может не быть вовсе — и у части официальных обоев WE, и у
    // обычных работ Мастерской. Тогда тип определяется по содержимому
    // каталога, как это делает сам Wallpaper Engine.
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

/// Видеофайл ли это. Список расширений ОДИН на весь продукт и живёт в ядре.
fn is_video_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let lower = path.to_string_lossy().to_lowercase();
    rexon_core::user_props::VIDEO_ENTRY_EXTS
        .iter()
        .any(|ext| lower.ends_with(ext))
}
