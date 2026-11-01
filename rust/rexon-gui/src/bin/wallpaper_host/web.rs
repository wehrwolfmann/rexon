// Режим «веб» хоста обоев: что показывать и с какими свойствами.
//
// Замена удалённого `python -m rexon.web_wallpaper`: тот же контракт
// окружения (REXON_WP_OUTPUT / WIDTH / HEIGHT / FPS), тот же mmap-файл для
// расширения оболочки, те же три способа положить окно на задний план.
// Разбор устройства и границ — src/web_host.rs.

use std::path::{Path, PathBuf};

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString};

use rexon_gui::wallpaper_host::{qml_value_to_variant, PLACEMENT_OFFSCREEN};
use rexon_gui::web_host::{init_web_engine, parse_args};

use crate::{variant_str, HostEnv, QmlPlan};

/// Имя модуля в таблице точек входа ядра (`rexon-core::qt_app`).
const ENTRY_MODULE: &str = "rexon.web_wallpaper";

/// Адрес QML внутри статического ресурса модуля `org.rexon.gui`.
const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperWeb.qml";

/// Метка строк журнала режима.
const TAG: &str = "REXON-WEB";

pub fn plan(argv: &[String]) -> Result<QmlPlan, String> {
    let cli = parse_args(argv)?;
    // Ссылка идёт как есть: канонизировать её нельзя, а `project.json` рядом
    // с ней взять неоткуда.
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

    // Подмешиваемый в страницу JS: заглушки Wallpaper Engine (до первого
    // скрипта страницы) и передача свойств обоев со следующим за ней тиком
    // звука (после загрузки документа). Все три текста — из ядра.
    properties.insert(
        QString::from("polyfillJs"),
        variant_str(rexon_core::wallpaper::WE_POLYFILL_JS),
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
    // ⚠️ Офскрин-режим настраивается ДО инициализации Chromium: переменная
    // читается при поднятии подсистемы, и после него её правка не делает
    // ничего. Платформу Qt к этому месту уже выставил общий `run_qml`.
    if plan.placement == PLACEMENT_OFFSCREEN
        && std::env::var_os("QTWEBENGINE_CHROMIUM_FLAGS").is_none()
    {
        // ⚠️ Именно `--in-process-gpu` и НИКАКОГО `--disable-gpu`. С
        // выключенным GPU Chromium не сводит холст (canvas2D/WebGL) в граф
        // сцены Qt: снятый с окна кадр содержал бы только DOM. Строку флагов
        // держит ядро.
        // SAFETY: зовётся из main до создания приложения Qt и до любых
        // потоков — читателей окружения ещё нет.
        unsafe {
            std::env::set_var(
                "QTWEBENGINE_CHROMIUM_FLAGS",
                rexon_core::wallpaper::WEB_OFFSCREEN_CHROMIUM_FLAGS,
            )
        };
    }
    // ⚠️ ПЕРВЫМ Qt-действием и ДО QGuiApplication: инициализация Chromium
    // выставляет общий контекст OpenGL всем окнам процесса, а после создания
    // приложения это уже поздно — WebEngineView встал бы пустым. И ТОЛЬКО в
    // этом режиме: библиотека открывается здесь, видео её не грузит.
    init_web_engine()
}

/// Локальный адрес: сам HTML, каталог обоев с `project.json` либо путь на
/// файл ВНУТРИ такого каталога.
fn from_local(raw: &str) -> Result<(String, serde_json::Map<String, serde_json::Value>), String> {
    let path = PathBuf::from(raw);
    let path = path.canonicalize().unwrap_or(path);

    if path.is_dir() {
        return from_wallpaper_dir(&path);
    }
    if !path.is_file() {
        // Ровно тот же текст, которым движок обоев отвечает покупателю.
        return Err(rexon_core::wallpaper_engine::web_missing_message(raw));
    }

    // Путь на файл ВНУТРИ каталога обоев: рядом может лежать `project.json`
    // со свойствами, и терять их из-за того, что человек ткнул в index.html,
    // а не в каталог, нельзя.
    let props = path
        .parent()
        .filter(|dir| dir.join("project.json").is_file())
        .map(project_props)
        .unwrap_or_default();
    Ok((path.to_string_lossy().into_owned(), props))
}

/// Каталог обоев с `project.json`: разбор, определение типа и честный отказ
/// для всего, что рисует не этот режим.
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

/// Свойства из соседнего `project.json`. Разбор не удался — не повод
/// остаться без обоев: свойства уедут пустыми, страница возьмёт свои.
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
