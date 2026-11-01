// Режим «клиент вывода waywallen» хоста обоев (`--mode waywallen`).
//
// Сцены Wallpaper Engine рисует демон waywallen; на X11 вне KDE у него нет
// своего клиента вывода, и этим клиентом становится хост: окна-рабочие
// столы с элементом вывода waywallen (разбор — qml/WallpaperWaywallen.qml и
// `rexon-core::wallpaper_backend`, раздел «дисплей для демона»).
//
// Запускают его двое, строкой одного вида (`--socket <путь> --mode
// waywallen`): демон — по определению вывода при своём старте, Rexon — по
// «Применить», когда дисплеев нет. Поэтому до Qt здесь две вещи:
//   * на Wayland и без `DISPLAY` хост только ждёт — окна X11 там легли бы
//     поверх стола, а выход демон счёл бы падением и перезапускал бы по кругу;
//   * один экземпляр на сеанс: второй ждёт блокировку и подхватывает вывод,
//     если первый умрёт. Двух окон на монитор не бывает.

use std::path::Path;

use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use rexon_core::wallpaper_backend as wb;
use rexon_core::waywallen_kde_plugin as wkp;
use rexon_gui::wallpaper_host::{PLACEMENT_X11, USAGE};

use crate::{variant_str, QmlPlan};

/// Имя модуля в таблице точек входа ядра (`rexon-core::qt_app`): окна этого
/// режима показывают сцены.
const ENTRY_MODULE: &str = "rexon.scene_wallpaper";

const QML_URL: &str = "qrc:/qt/qml/org/rexon/gui/qml/WallpaperWaywallen.qml";

/// Метка строк журнала режима.
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

/// Обёртка элемента вывода из пакета `org.waywallen.kde`: вшитый в Rexon
/// первым, установленный Plasma-плагин — запасным.
fn find_display_item() -> Option<String> {
    let is_file = |p: &str| Path::new(p).is_file();
    let data_dir = rexon_core::installer::pick_package_data_dir(&rexon_core::cli::repo_root(), &is_file);
    let home = std::env::var("HOME").unwrap_or_default();
    wkp::display_package_candidates(data_dir.as_deref(), &home)
        .iter()
        .filter_map(|base| wkp::find_runtime_so(base, Some(wkp::RUNTIME_SO_REL)))
        .filter_map(|so| wkp::display_item_qml_of(&so))
        .find(|qml| is_file(qml))
}

/// Ждать вечно, ничего не показывая (Wayland, нет `DISPLAY`, вывод уже
/// показывает другой экземпляр). Процесс уходит по сигналу демона.
fn idle_forever() -> ! {
    loop {
        std::thread::park();
    }
}

pub fn plan(argv: &[String]) -> Result<QmlPlan, String> {
    let args = parse(argv)?;
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    if !args.windowed && wb::rexon_host_should_idle(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"), &env("DISPLAY")) {
        rexon_core::log_info!(
            "wallpaper-host",
            "{TAG} сеанс не X11 (XDG_SESSION_TYPE={}, DISPLAY={}) — вывод здесь не мой, жду",
            env("XDG_SESSION_TYPE"),
            env("DISPLAY")
        );
        idle_forever();
    }

    // Один экземпляр на сеанс. Блокировка держится до конца процесса.
    let runtime = env("XDG_RUNTIME_DIR");
    if !runtime.is_empty() && !args.windowed {
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

    let Some(item) = find_display_item() else {
        return Err("нет элемента вывода waywallen (пакет org.waywallen.kde в данных Rexon) — выводить нечем".to_string());
    };
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

/// Элементу вывода нужен EGL: кадры демона приходят буферами DMA-BUF, а их
/// импорт в Qt на X11 идёт только через EGL (README waywallen-display о
/// plasmashell на X11). По умолчанию Qt на xcb берёт GLX. Платформу Qt
/// здесь не трогаем — её выставляет общий `prepare_qpa_platform` в main.rs.
fn before_app(_plan: &QmlPlan) -> Result<(), String> {
    const GL_INTEGRATION: &str = "QT_XCB_GL_INTEGRATION";
    if std::env::var_os(GL_INTEGRATION).is_none_or(|v| v.is_empty()) {
        // SAFETY: зовётся из main до создания приложения Qt и до любых
        // потоков — читателей окружения ещё нет.
        unsafe { std::env::set_var(GL_INTEGRATION, "xcb_egl") };
    }
    Ok(())
}
