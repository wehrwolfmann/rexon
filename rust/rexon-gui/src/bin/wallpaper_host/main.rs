// rexon-wallpaper-host — ОДИН хост обоев Rexon без Python в процессе.
//
// Видео (QtMultimedia) и веб (QtWebEngine) — режимы одного бинаря, а не два
// хоста: 15.09.2026 владелец решил хосты не плодить. Строку запуска собирает
// ядро (`rexon-core::wallpaper_engine::wallpaper_host_argv`), режим задаёт
// ключ `--mode`. Здесь — общий подъём Qt; что показывать, решают `video.rs`
// и `web.rs`, разбор ключей режимов — src/wallpaper_host.rs и src/web_host.rs.
//
// Запуск:
//     rexon-wallpaper-host --mode video /путь/видео.mp4 [--no-audio]
//     rexon-wallpaper-host --mode video /каталог/обоев             # с project.json
//     rexon-wallpaper-host --mode web https://example.com
//     rexon-wallpaper-host --mode web /путь/index.html --windowed  # окно 1280×720
//     rexon-wallpaper-host --mode swf /путь/ролик.swf
//     rexon-wallpaper-host /путь/видео.mp4     # без --mode — видео (прежний контракт)
//     rexon-wallpaper-host --mode waywallen [--socket <display.sock>]  # вывод демона на X11
// Самопроверка (отчитывается и закрывается сам, код возврата — приговор):
//     rexon-wallpaper-host --mode web /путь/index.html --selftest
//
// ⚠️ Ленивость режимов. Chromium поднимается ТОЛЬКО веб-режимом: библиотека
// QtWebEngineQuick открывается `dlopen` из `web.rs` (разбор —
// src/web_host.rs), в таблице зависимостей бинаря её нет, и видео-обои не
// платят за неё при каждом старте. QtMultimedia грузит сам QML видео.
//
// Третий режим — Flash (SWF, Ruffle через wgpu, крейт `rexon-swf`): у него
// не QML, а свой цикл отрисовки, поэтому в `run_qml` он не заходит, а
// возвращает код сам (`swf.rs`). Коды возврата — из той же таблицы ядра
// (`wallpaper_engine::host_exit`). Vulkan и Ruffle поднимаются ТОЛЬКО этим
// режимом: видео и веб за них при старте не платят.

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
// Ссылка на библиотеку крейта обязательна, а не декоративна: без неё rustc
// не подтягивает rlib, а вместе с ним и статические архивы cxx-qt (moc,
// QML-плагин с ресурсами) — бинарь соберётся, а QML будет пуст.
use rexon_gui::wallpaper_audio::qobject::WallpaperAudio;
use rexon_gui::wallpaper_host::qobject::WallpaperPlacement;

fn main() {
    // ⚠️ ПЕРВОЙ строкой, до всякого Qt. Паника хоста обоев не имеет права
    // раскручивать стек: раскрутка идёт через разбор Qt, а он на X11 ждёт
    // поток очереди событий — на этом ожидании хост однажды завис навсегда
    // (разбор — `wallpaper_host::verdict`). Обои на всех режимах поднимает
    // заново исполнитель, а вечно живой пустой процесс не поднимет никто.
    install_panic_guard();
    std::process::exit(run());
}

/// Коды возврата — общая таблица ядра `wallpaper_engine::host_exit`: 1 —
/// нечего показывать, 2 — QML не поднялся, 3 — поток кадров не пошёл; у
/// видео ещё 4 (звук просили, дорожки нет), у веба 5 (страница не
/// загрузилась) — оба выставляет самопроверка из QML; у Flash 7 (нет
/// Vulkan-видеокарты для обоев) и 8 (ролик не загрузился).
fn run() -> i32 {
    // Язык интерфейса Rexon — до всего остального: имя звукового потока
    // обоев в системном микшере берётся из каталога перевода
    // (`wallpaper_audio::stream_description`).
    rexon_core::i18n::init();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mode, rest) = match take_mode(&argv) {
        Ok(taken) => taken,
        Err(message) => return refuse(&message),
    };
    // Журнал — до разбора режима: строки `-START` и отказы плана уже
    // события. Справка и неверные ключи выше — вывод команды, не журнал.
    install_host_log(rest.iter().any(|a| a == "--selftest" || a.starts_with("--selftest=")));
    let planned = match mode {
        HostMode::Video => video::plan(&rest),
        HostMode::Web => web::plan(&rest),
        // Клиент вывода демона waywallen на X11 (сцены Wallpaper Engine).
        HostMode::Waywallen => waywallen::plan(&rest),
        // Flash рисует wgpu, а не QML: свой цикл, свои коды той же таблицы.
        // Платформу Qt выставляет этот файл — то же общее решение, что у
        // QML-режимов в `run_qml`.
        HostMode::Swf => {
            return match swf::plan(&rest) {
                Ok(plan) => {
                    prepare_qpa_platform(plan.placement, HostMode::Swf);
                    swf::run(plan)
                }
                Err(code) => code,
            };
        }
    };
    match planned {
        Ok(plan) => run_qml(plan),
        Err(message) => refuse(&message),
    }
}

/// Отказ до всякого Qt: справка — не ошибка; всё остальное — нечего показывать.
fn refuse(message: &str) -> i32 {
    eprintln!("{message}");
    if message == USAGE {
        host_exit::OK
    } else {
        host_exit::NOTHING_TO_SHOW
    }
}

/// Всё, что нужно окну режима, посчитанное ДО первого вызова Qt.
pub struct QmlPlan {
    /// Имя модуля в таблице точек входа ядра (`rexon-core::qt_app`). Своей
    /// строки с именем приложения у режимов нет: без `desktopFileName` KDE
    /// на Wayland рисует в панели задач дефолтный логотип вместо иконки.
    pub entry_module: &'static str,
    /// Адрес QML внутри статического ресурса модуля `org.rexon.gui`.
    pub qml_url: &'static str,
    /// Метка строк журнала режима (`REXON-WPH` у видео, `REXON-WEB` у веба)
    /// — прежние метки двух хостов сохранены: по ним ищут в журналах.
    pub tag: &'static str,
    /// Режим хоста: от него зависит, чем снимается кадр в режиме потока и,
    /// значит, на какой платформе Qt жить (`wallpaper_mmap::host_stream_capture`).
    pub mode: HostMode,
    pub placement: &'static str,
    pub properties: QMap<QMapPair_QString_QVariant>,
    pub selftest: bool,
    /// Действие режима ДО `QGuiApplication` (роль звукового потока у видео,
    /// подъём Chromium у веба). Ошибка — «QML не поднялся».
    pub before_app: fn(&QmlPlan) -> Result<(), String>,
}

fn run_qml(plan: QmlPlan) -> i32 {
    // ⚠️ ДО создания QGuiApplication: платформу Qt выбирает конструктор
    // приложения. Без этого на GNOME/Cinnamon-Wayland окно обоев в режиме
    // потока кадров выходило обычным окном поверх рабочего стола. И ДО
    // инициализации Chromium: он читает платформу при подъёме.
    prepare_qpa_platform(plan.placement, plan.mode);

    // Без этого прогон из скрипта выглядит немым: когда stderr не терминал,
    // Qt на Arch/CachyOS уводит сообщения в journald, и отчёт самопроверки
    // из QML в вывод не попадает.
    if plan.selftest {
        unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    }

    if let Err(message) = (plan.before_app)(&plan) {
        rexon_core::log_error!("wallpaper-host", "{}-ERROR {message}", plan.tag);
        return host_exit::QML_FAILED;
    }

    // ОБЯЗАТЕЛЬНО: статический QML-плагин с ресурсами и регистрацией типов
    // линкуется отдельным архивом, и без явной ссылки линкер его
    // выбрасывает — QML тогда падает на «qrc:/… No such file or directory»
    // ещё до всякой ошибки импорта.
    cxx_qt::init_qml_module!("org.rexon.gui");

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        apply_qt_identity(app, plan.entry_module);
    }
    // ⚠️ СРАЗУ после приложения и до первого окна. Окон у хоста столько,
    // сколько мониторов; отключённый монитор уносит своё окно, и умолчание
    // Qt «последнее окно закрыли — выходим» убило бы весь процесс обоев на
    // ровном месте (см. `wallpaper_host::keep_alive_without_windows`).
    keep_alive_without_windows();

    let tag = plan.tag;
    let mut engine = QQmlApplicationEngine::new();
    let Some(mut engine) = engine.as_mut() else {
        rexon_core::log_error!("wallpaper-host", "{tag}-ERROR движок QML не создался");
        return verdict(host_exit::QML_FAILED);
    };

    // Свойства едут корню QML начальными свойствами: контекстные свойства в
    // cxx-qt-lib 0.9 не обёрнуты, а setInitialProperties есть и применяется
    // ДО первой привязки — то есть до того, как окно решит, каким ему быть.
    engine.as_mut().set_initial_properties(&plan.properties);

    // Приговор «QML не загрузился» выносится сигналом, а не молчанием: без
    // него пустой движок дал бы мгновенный выход кодом 0, и «обоев нет»
    // выглядело бы как «всё хорошо».
    // Arc/AtomicBool, а не Rc/Cell: cxx-qt требует от обработчика сигнала
    // Send — сигнал вправе прийти не из главного потока.
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

    // Приговор через `verdict`, а не голым числом: сразу за этой строкой
    // начинается разбор Qt, и он обязан уложиться в срок (разбор — там же).
    let code = verdict(match app.as_mut() {
        Some(app) => app.exec(),
        None => host_exit::QML_FAILED,
    });
    // Проигрыватель — стоп и пустой источник ДО разбора движка и окон, уже
    // под сроком `verdict`. Иначе окно сносится, пока поток декодирования
    // ещё работает, — тот же класс краха, что у главного окна на выходе
    // (SIGSEGV в `av_log`, разбор — в `bin/main_window.rs` у того же
    // вызова). Выход здесь бывает по `Qt.quit()` отладочного окна.
    unsafe { rexon_stop_media_players() };
    code
}

// Шим — `cpp/rexon_gui_factory.cpp`, общий с главным окном.
extern "C" {
    fn rexon_stop_media_players() -> std::ffi::c_int;
}

/// Как приложение представляется системе — из общей таблицы точек входа.
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

/// Окружение показа — ОДНО на все режимы: куда писать кадры, какого они
/// размера, с какой частотой и каким способом класть окно. Все решения —
/// ядра, своих здесь нет. Прежде каждый хост читал его своей копией.
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
        // Способ показа у всех режимов обязан совпадать, иначе на одном
        // рабочем столе видео ушло бы в слой, а веб — в окно поверх всего.
        let placement = host_placement(mmap_output.is_some(), windowed, wayland);
        HostEnv {
            mmap_output,
            mmap_w,
            mmap_h,
            fps,
            placement,
        }
    }

    /// Свойства корня QML, общие для всех режимов, и честные отказы до
    /// всякого Qt.
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
        // Офскрин без файла кадров — это молча чёрный экран у человека,
        // поэтому отказ честный и до всякого Qt.
        if self.placement == PLACEMENT_OFFSCREEN && self.mmap_output.is_none() {
            return Err("офскрин-режим без REXON_WP_OUTPUT: кадры некуда писать".to_string());
        }
        Ok(())
    }
}

pub fn variant_str(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

// Типы упоминаются ради линковки статических архивов cxx-qt: без обращения
// к ним QML-плагин крейта выбрасывается линкером, и `WallpaperPlacement` в
// QML не существует.
#[allow(dead_code)]
fn keep_qml_types_linked() -> (&'static str, &'static str) {
    (
        std::any::type_name::<WallpaperPlacement>(),
        std::any::type_name::<WallpaperAudio>(),
    )
}
