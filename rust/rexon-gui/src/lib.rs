// rexon-gui — GUI-слой Rexon на Rust: QObject'ы и модели списков для QML
// через cxx-qt. Целевая картина миграции (решение 2026-07-29): Rust-ядро +
// cxx-qt + существующий QML, уходит только PySide6.
//
// Этот крейт — фундамент замены Python-моста
// rexon/qt_wallpaper_engine/qml_ui/bridge.py.
//
// Логики здесь нет и не будет: модели только раскладывают по ролям то, что
// посчитало rexon-core. Это условие проверяемости — ядро тестируется без Qt.

// ⚠️ ССЫЛКА НИЖЕ ОБЯЗАТЕЛЬНА, И ЭТО НЕ ФОРМАЛЬНОСТЬ. QApplication на Rust
// живёт в cxx-qt-lib-extras, и пользуется ей ОДИН бинарь — главное окно.
// Но инициализатор крейта, который cxx-qt-build вписывает в rexon-gui, зовёт
// инициализатор extras ВСЕГДА, а его статический архив линкуется только к
// тем артефактам, чей Rust-код на крейт ссылается. Без этой строки падала
// линковка ВСЕГО пакета — и чужих бинарей тоже (`we-socket-probe`,
// `rexon-wallpaper-host`), с «undefined symbol:
// cxx_qt_init_crate_cxx_qt_lib_extras», причём на `cargo build --lib` этой
// поломки не видно вовсе. Убирать нельзя, даже если «никто не использует».
use cxx_qt_lib_extras as _;

unsafe extern "C" {
    /// Регистрация типов в `waywallen.ui` (cpp/rexon_register_we.cpp).
    fn rexon_do_register_we_types();
}

/// Зарегистрировать типы вкладки обоев в `waywallen.ui`.
///
/// Обычный `extern "C"`, а не мост cxx: регистрация не передаёт и не
/// возвращает ничего, генератору тут работы нет.
///
/// Звать ОДИН раз, до загрузки QML, из главного потока с живым
/// `QGuiApplication` (`we_env::prepare_process`). Порядок важен: сначала
/// регистрация всех типов, потом создание движка QML, иначе движок встаёт
/// в `QQmlThread` на дедлоке глобальной регистрации Qt-Protobuf.
pub fn register_we_types() {
    unsafe { rexon_do_register_we_types() }
}

/// Смена скина на лету: один реестр на все страницы QML.
///
/// ⚠️ Зачем вообще. Вкладки на виджетах перекрашиваются в тот же миг, когда
/// покупатель выбрал скин в «Настройках» (`ui_base.apply_skin` →
/// `QApplication.setPalette`). Перенесённые вкладки на QML брали палитру
/// свойством `palette`, объявленным `CONSTANT`, — сигнала об изменении не
/// было вовсе, привязки не пересчитывались, и открытая вкладка оставалась
/// в цветах прошлого скина до переоткрытия. Половина окна в новом скине,
/// половина в старом — это покупатель считает поломкой (правило #13).
///
/// ⭐ Почему реестр, а не по правке в каждой странице (правило #12).
/// Страницы одинаково берут цвета (`ui_base::active_skin_colors`), и
/// одинаково же в них жил прошлый дефект с запечённым скином по умолчанию
/// (коммит 3ee6011). Здесь механизм ОДИН: страница подписывается одной
/// строкой в `Initialize` и снимается одной в `Drop`, а следующая
/// перенесённая вкладка получает перекраску теми же двумя строками — не
/// пятой копией рассылки.
///
/// Замыкание, а не `CxxQtThread<T>` конкретного типа: подписаны РАЗНЫЕ
/// типы страниц, а обобщить хранение по типу нельзя — cxx-qt печатает свой
/// алиас на каждый QObject. Тот же приём, что у реестра прогресса
/// (`query_base::progress`) и склада приобретений (`remote_list_model`).
pub mod skin {
    use core::pin::Pin;
    use std::sync::{Mutex, OnceLock};

    /// Толчок одной подписанной странице. `Err(())` — объект умер, запись
    /// пора снять.
    type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Notify)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Подписать живую страницу на смену скина. Возвращает номер записи —
    /// его страница обязана снять в `Drop`.
    ///
    /// `emit` — сигнал `paletteChanged` этой страницы. Доставка идёт через
    /// `CxxQtThread::queue`: на уже умершем объекте она возвращает
    /// типизированную ошибку `ObjectDestroyed`, а не падает, поэтому ни
    /// `unwrap()`, ни предварительного `is_destroyed()` здесь нет (между
    /// проверкой и постановкой объект успевает умереть).
    pub fn watch<T>(handle: cxx_qt::CxxQtThread<T>, emit: fn(Pin<&mut T>)) -> u64
    where
        T: cxx_qt::Threading + 'static,
    {
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((
            id,
            Box::new(move || match handle.queue(emit) {
                Ok(()) => Ok(()),
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }),
        ));
        id
    }

    /// Снять подписку. Идемпотентно: `0` — страница не подписывалась.
    pub fn unwatch(id: u64) {
        if id == 0 {
            return;
        }
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// Сколько страниц сейчас подписано (для проверок).
    pub fn watching() -> usize {
        registry()
            .lock()
            .expect("реестр скина не отравлен")
            .entries
            .len()
    }

    /// Толкнуть все живые страницы: скин сменился, палитру пересчитать.
    /// Возвращает число оповещённых; мёртвые записи снимаются заодно.
    pub fn dispatch() -> usize {
        let mut told = 0usize;
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.entries.retain(|(_, notify)| match notify() {
            Ok(()) => {
                told += 1;
                true
            }
            Err(()) => false,
        });
        told
    }
}

/// Одна дверь к всплывающей плашке состояния для всех вкладок.
///
/// ⭐ Почему здесь, а не по копии в каждой странице (правило #12). Дефект
/// «сообщение не гаснет никогда» заводился ТРИЖДЫ — в виджетных «Настройках»
/// (28.06), в виджетном «Steam» (08.07) и в третий раз в QML-вкладке «Steam»,
/// где живая проверка 18.09 застала плашку висящей 29 минут. Разметка у всех
/// вкладок теперь одна (`qml/RexonToast.qml`), и питает её этот модуль:
/// пока краски, поля, срок жизни и ширина приходят отсюда, у вкладки не
/// может завестись ни своя плашка, ни свои числа.
///
/// Считает всё равно ядро (`ui_base`) — здесь только перевод его ответов в
/// типы Qt.
pub mod toast {
    use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};
    use rexon_core::ui_base;

    /// Краски и метрики плашки одной картой: QML читает её точкой
    /// (`styles.toast.paddingH`). Та же дверь, что у виджетной плашки —
    /// `ui_base::toast_colors` и константы `TOAST_*` рядом с ней.
    pub fn style_map(dark: bool) -> QMap<QMapPair_QString_QVariant> {
        let skin = ui_base::toast_colors(dark);
        let col = |css: &str| {
            QVariant::from(&QString::from(ui_base::css_color_to_qml(css).as_str()))
        };
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in [
            ("background", col(skin.background)),
            ("border", col(skin.border)),
            ("textColor", col(skin.text)),
            ("borderWidth", QVariant::from(&ui_base::TOAST_BORDER_WIDTH)),
            ("radius", QVariant::from(&ui_base::TOAST_RADIUS)),
            ("paddingH", QVariant::from(&ui_base::TOAST_PADDING_H)),
            ("paddingV", QVariant::from(&ui_base::TOAST_PADDING_V)),
            ("margin", QVariant::from(&ui_base::TOAST_MARGIN)),
        ] {
            out.insert(QString::from(key), value);
        }
        out
    }

    /// Через сколько сообщение гаснет само, мс.
    pub fn timeout_ms() -> i32 {
        ui_base::TOAST_TIMEOUT_MS as i32
    }

    /// Ширина плашки под текст такой ширины.
    ///
    /// Виджетная плашка меряет текст в одну строку и прибавляет к нему СВОИ
    /// поля: у QLabel и рамка, и внутренние поля приходят таблицей стилей и
    /// входят в поля содержимого. У прямоугольника QML их нет, поэтому те же
    /// числа прибавляются руками — из ядра, не свои.
    pub fn width(page_width: i32, text_width: i32) -> i32 {
        let frame = 2 * (ui_base::TOAST_PADDING_H + ui_base::TOAST_BORDER_WIDTH);
        ui_base::toast_width(
            page_width.max(0),
            text_width.max(0) + frame + ui_base::TOAST_TEXT_SLACK,
            0,
            ui_base::TOAST_MAX_WIDTH,
        )
    }
}

// ── Перевод строк QML: тот же каталог `.mo`, что у ядра ─────────────────

/// Приёмник строки — сигнатура `RexonTextSink` из `cpp/rexon_gui_factory.h`.
type TextSink = unsafe extern "C" fn(target: *mut std::ffi::c_void, utf8: *const u8, len: i32);

/// Поиск строки — сигнатура `RexonAppTranslate`.
type AppTranslate = unsafe extern "C" fn(
    disambiguation: *const std::ffi::c_char,
    source: *const std::ffi::c_char,
    n: i32,
    sink: TextSink,
    target: *mut std::ffi::c_void,
) -> bool;

unsafe extern "C" {
    fn rexon_install_app_translator(context: *const std::ffi::c_char, lookup: Option<AppTranslate>) -> bool;
}

/// Колбэк переводчика окна: строка QML → `i18n::qt_translate`.
///
/// Qt зовёт его из любого потока, в том числе из потока загрузки QML;
/// ядро читает каталог под `RwLock` и ничего не ждёт.
unsafe extern "C" fn app_translate(
    disambiguation: *const std::ffi::c_char,
    source: *const std::ffi::c_char,
    n: i32,
    sink: TextSink,
    target: *mut std::ffi::c_void,
) -> bool {
    if source.is_null() {
        return false;
    }
    // Исходники QML — UTF-8 (так их отдаёт движок `qsTranslate`); строка не
    // в UTF-8 в каталоге не найдётся всё равно.
    let Ok(source) = unsafe { std::ffi::CStr::from_ptr(source) }.to_str() else {
        return false;
    };
    let disambiguation = if disambiguation.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(disambiguation) }.to_str().ok()
    };
    let Some(text) = rexon_core::i18n::qt_translate(disambiguation, source, n) else {
        return false;
    };
    let Ok(len) = i32::try_from(text.len()) else {
        return false;
    };
    unsafe { sink(target, text.as_ptr(), len) };
    true
}

/// Поставить переводчик строк QML (`qsTranslate("rexon", …)`) на язык,
/// который ядро уже выбрало (`i18n::init`). Отдельного каталога для QML
/// нет: переводчик спрашивает ядро, ядро — тот же `.mo`, что и для строк
/// на Rust. У английского (исходного) каталога нет — прежний переводчик
/// только снимается.
///
/// → false, если приложения Qt ещё нет. Звать из главного потока после
/// создания приложения и до загрузки QML; после смены языка ядра — заново
/// (у переводчика кэш строк).
pub fn install_app_translator() -> bool {
    let context = std::ffi::CString::new(rexon_core::i18n::QT_CONTEXT).expect("контекст без нуля");
    let lookup: Option<AppTranslate> =
        rexon_core::i18n::mo_path().is_some().then_some(app_translate as AppTranslate);
    // Переводчик копирует контекст себе (QByteArray), строка может умереть.
    unsafe { rexon_install_app_translator(context.as_ptr(), lookup) }
}

/// Объект побочных действий вкладки «Оформление»: магазин, обзор диска,
/// картинки, установка, применение и удаление тем. До него страница
/// получала `actions` только от Python.
pub mod animations_actions;
pub mod app_widgets_actions;
pub mod appearance_actions;
pub mod appearance_page;
pub mod compare_page;
pub mod crash_report;
pub mod desktops_actions;
pub mod desktops_page;
pub mod extensions_actions;
/// Мастер первого запуска для окна на Rust: решение ядра «предлагать ли
/// установку», подписи мастера, фоновая установка и «больше не спрашивать».
/// `request_show` — дверь кнопки «Установить Rexon в систему…» из «Настроек».
pub mod first_run_wizard;
pub mod extensions_page;
pub mod discover_queries;
pub mod display_queries;
pub mod layer_shell;
pub mod local_preview_render;
pub mod layer_shell_selftest;
/// Хозяин главного окна на Rust: `ApplicationWindow` вместо `QMainWindow`.
pub mod main_window;
/// Обучение при первом запуске для окна на Rust; `request_replay` — дверь
/// кнопки «Пройти обучение заново».
pub mod onboarding_overlay;
pub mod query_base;
/// Общий движок хранилищ выбора: база `SelectStorage` и надстройка
/// `WallpaperSelectStorage` одним макросом на оба моста. Не мост cxx-qt.
pub mod select_storage_base;
/// Объект побочных действий вкладки «Настройки»: снимки машины, скин,
/// автозапуск, файловый менеджер, родные приложения окружения, звуки
/// событий, защита приложений по умолчанию. До него страница получала
/// `actions` только от Python.
pub mod settings_actions;
pub mod settings_page;
/// Фоновые задачи вкладки «Steam»: порт пяти `QThread` оригинала на async
/// (tokio) — инвентарь, метки заставок, кадры-превью, миниатюры и
/// применение фона обоями. Не мост cxx-qt: Qt здесь нет вовсе.
pub mod steam_workers;
/// Объект побочных действий вкладки «Steam»: снимок витрины заставок, выбор
/// своего ролика, звёздочки, три переключателя, полноэкранный показ и токен
/// WebAPI. До него страница получала `actions` только от Python.
pub mod steam_actions;
/// Подвкладка «Витрины профиля»: нарезка картинки под витрину профиля Steam.
pub mod steam_showcase_actions;
/// Вкладка «Steam» — вкладка класса B на QML.
pub mod steam_page;
/// Проба «64-битное целое в QML»: живой движок против мостов. Не мост
/// cxx-qt — стенд, как `layer_shell_selftest`. Статический напарник —
/// сторож `qml_int64` в конце этого файла.
pub mod qml_int64_probe;
pub mod qvalue;
pub mod remote_list_model;
pub mod tag_queries;
pub mod user_property_model;
pub mod wallpaper_actions;
pub mod wallpaper_audio;
pub mod wallpaper_host;
pub mod web_host;
/// Кнопки «послушать» тем звуков: пять звуков, Магазин до установки.
pub mod sound_preview_actions;
pub mod window_buttons_actions;
/// «Отклик при запуске»: курсор и панель задач, пока программа запускается.
pub mod launch_feedback_actions;
/// «Цвет акцента» над сеткой «Цветовых схем».
pub mod accent_actions;
/// «Светлое и тёмное» по расписанию над «Глобальными темами» и «Цветовыми схемами».
pub mod day_night_actions;
/// «Сохранить текущее оформление…» над «Глобальными темами».
pub mod global_save_actions;
pub mod dde_look_actions;
pub mod mate_panel_actions;
pub mod window_menu;
pub mod wallpaper_list_model;
pub mod wallpaper_page_golden;
pub mod wallpaper_page_selftest;
pub mod wallpaper_queries;
pub mod wallpaper_select_storage;
pub mod autostart_queries;
pub mod playlist_queries;
pub mod playlist_item_select_storage;
pub mod plugin_queries;
pub mod plugin_translations;
pub mod remote_store_queries;
pub mod source_queries;
pub mod settings_queries;
pub mod status_queries;
pub mod global_pause_queries;
pub mod thumbnail_request;
/// Значок Rexon в системном лотке. НЕ мост cxx-qt: QML здесь ни при чём,
/// в `.files()` в build.rs этому файлу места нет — только шим
/// `cpp/rexon_tray.cpp` в `.cpp_files()`.
pub mod tray;
/// Мост «не гасить экран» (org.freedesktop.ScreenSaver). НЕ мост cxx-qt:
/// только шим `cpp/rexon_screensaver_bridge.cpp` в `.cpp_files()`.
pub mod screensaver_bridge;
pub mod we_env;
pub mod we_transport;
pub mod we_action;
pub mod we_actions;
pub mod we_app;
pub mod we_daemon_dbus;
pub mod we_notify;
pub mod we_objmodel;
/// Шелл вкладки «Wallpaper Engine» на QML: рейл и место вендорной страницы.
// Скрытая страница «Разработчик» вкладки обоев: обновления upstream и
// сверка паритета UI.
pub mod we_dev_page;
pub mod we_page;
/// Фоновые задачи страницы «Разработчик» вкладки WE: порт трёх `QThread`
/// оригинала на async (tokio) — проверка upstream, установка AppImage движка
/// и плагин-архива OWE. Не мост cxx-qt: Qt здесь нет вовсе.
pub mod we_update_workers;
pub mod we_util;
pub mod we_singletons;
pub mod we_socket;

/// Сторож состава сборки: списки в `build.rs` против содержимого каталогов.
///
/// ЗАЧЕМ ТЕСТ, А НЕ ПРОВЕРКА В `build.rs`. Забытая строка проявляется хуже
/// всего именно тогда, когда файл ТОЛЬКО ЧТО добавлен: в этом случае
/// `build.rs` может не перезапуститься вовсе, и любая проверка внутри него
/// промолчит. Тест выполняется при каждом `cargo test -p rexon-gui --lib`
/// независимо от того, сочла ли cargo сборочный скрипт устаревшим, и
/// перечитывает каталоги заново.
///
/// Сверка идёт со СЛЕПКОМ того, что `build.rs` реально отдал в
/// `.files()` / `qml_files()` (файлы в OUT_DIR), а не с текстом `build.rs`:
/// разбирать исходник заново — значит проверять похожесть, а не факт.
#[cfg(test)]
mod toast_tests {
    use super::toast;
    use rexon_core::ui_base;

    /// Срок жизни сообщения приходит из ядра и положителен.
    ///
    /// ⭐ Сторож против третьего рецидива: плашка, которой достался ноль,
    /// не гаснет вовсе — а именно так вкладка «Steam» и держала сообщение
    /// 29 минут. Разметка ноль трактует честно («не гаснуть»), поэтому
    /// стеречь надо здесь, у источника числа.
    #[test]
    fn toast_timeout_comes_from_the_core_and_is_positive() {
        assert_eq!(toast::timeout_ms(), ui_base::TOAST_TIMEOUT_MS as i32);
        assert!(toast::timeout_ms() > 0, "плашка с нулевым сроком не гаснет");
    }

    /// Ширина плашки — расчёт ядра, и в окно она помещается всегда.
    #[test]
    fn toast_width_stays_inside_the_page() {
        // Узкая страница: плашка не вылезает за её край.
        assert!(toast::width(300, 4000) <= 300);
        // Широкая страница: выше потолка ядра плашка не растёт.
        assert!(toast::width(4000, 4000) <= ui_base::TOAST_MAX_WIDTH);
        // Отрицательные числа приходят от QML на первом кадре (размера ещё
        // нет) и не должны давать отрицательной ширины.
        assert!(toast::width(-1, -1) >= 0);
    }
}

#[cfg(test)]
mod build_manifest {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    /// Мосты, ушедшие в `.files()`.
    const BRIDGE_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bridge_files.list"));
    /// Файлы, ушедшие в `qml_files()`.
    const QML_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/qml_files.list"));

    /// Разобрать слепок в набор путей относительно корня крейта.
    fn listed(manifest: &str) -> BTreeSet<String> {
        manifest
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Имена файлов с заданным расширением в каталоге крейта, без обхода
    /// вложенных каталогов: и мосты, и .qml лежат ровно одним уровнем
    /// (для мостов это требование qmltyperegistrar, см. `build.rs`).
    fn files_in(dir: &str, ext: &str) -> Vec<PathBuf> {
        let path = crate_dir().join(dir);
        let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("не читается каталог {}: {e}", path.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == ext))
            .collect();
        found.sort();
        found
    }

    /// Есть ли в файле настоящий мост cxx-qt.
    ///
    /// Мерило — атрибут в начале строки. Упоминание `cxx_qt::bridge` в
    /// комментарии мостом не делает: так, `src/query_base.rs` объясняет в
    /// шапке, почему он мостом НЕ является, и в `.files()` ему не место.
    fn is_bridge(path: &PathBuf) -> bool {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("не читается {}: {e}", path.display()));
        text.lines()
            .any(|line| line.trim_start().starts_with("#[cxx_qt::bridge"))
    }

    /// Пути, которые перечислены в `build.rs`, но которых нет на диске.
    fn missing_on_disk(listed: &BTreeSet<String>) -> Vec<String> {
        listed
            .iter()
            .filter(|rel| !crate_dir().join(rel).is_file())
            .cloned()
            .collect()
    }

    #[test]
    fn every_bridge_is_listed_in_build_rs() {
        let listed = listed(BRIDGE_MANIFEST);
        let forgotten: Vec<String> = files_in("src", "rs")
            .into_iter()
            .filter(is_bridge)
            .map(|p| format!("src/{}", p.file_name().unwrap().to_string_lossy()))
            .filter(|rel| !listed.contains(rel))
            .collect();
        assert!(
            forgotten.is_empty(),
            "мост cxx-qt не перечислен в .files() в rust/rexon-gui/build.rs: {}. \
             Допишите туда эти строки — иначе вкладка мёртвая: C++-половина \
             моста не собирается, и его типы не доходят до QML (прежний случай — \
             «undefined symbol: …cxxQtThreadQueue…» у живого Rexon)",
            forgotten.join(", ")
        );

        let stale = missing_on_disk(&listed);
        assert!(
            stale.is_empty(),
            "в .files() в rust/rexon-gui/build.rs перечислены несуществующие файлы: {}. \
             Уберите строки или верните файлы на место",
            stale.join(", ")
        );
    }

    #[test]
    fn every_qml_file_is_listed_in_build_rs() {
        let listed = listed(QML_MANIFEST);
        let forgotten: Vec<String> = files_in("qml", "qml")
            .into_iter()
            .map(|p| format!("qml/{}", p.file_name().unwrap().to_string_lossy()))
            .filter(|rel| !listed.contains(rel))
            .collect();
        assert!(
            forgotten.is_empty(),
            "файл .qml не перечислен в qml_files() в rust/rexon-gui/build.rs: {}. \
             Допишите туда эти строки — иначе сборка зелёная, а страница мёртвая: \
             файл не попадёт в запечённый ресурс модуля, и QML найдёт его только \
             в работе, отказом «qrc:/qt/qml/org/rexon/gui/…: No such file or directory»",
            forgotten.join(", ")
        );

        let stale = missing_on_disk(&listed);
        assert!(
            stale.is_empty(),
            "в qml_files() в rust/rexon-gui/build.rs перечислены несуществующие файлы: {}. \
             Уберите строки или верните файлы на место",
            stale.join(", ")
        );
    }
}

/// Сторож «64-битное целое наружу в QML»: мосты против списка исключений.
///
/// ЗАЧЕМ. cxx-qt печатает `i64`/`u64` как `::std::int64_t` /
/// `::std::uint64_t`, а на 64-битном Linux это `long` / `unsigned long` —
/// ОТДЕЛЬНЫЕ метатипы Qt. Движок QML их не знает, и литеральная
/// декларативная привязка в такое свойство («displayId: 7») отказывает
/// вместе со ВСЕМ документом: страница не создаётся вовсе.
///
/// ⚠️ ПОЧЕМУ ТЕСТ, А НЕ КОМПИЛЯТОР. Дефект невидим целиком: и сборка, и
/// `cargo test`, и гейт типов `scripts/test_we_rust_types.py` сходятся —
/// сигнатуры-то верные. Ломается только на экране, и только у
/// декларативной привязки. Живой замер, разбор и повторяемая проба —
/// `src/qml_int64_probe.rs`; здесь — статическая половина, которая не даёт
/// такому объявлению завестись СНОВА.
///
/// Разбираются ТОЛЬКО блоки `extern "RustQt"` — это и есть поверхность,
/// которую cxx-qt печатает в метаобъект. Блоки `extern "C++"` — обычные
/// вызовы C++ мимо Qt (`rexon_we_network_cache_size() -> i64`), их 64-битные
/// типы к QML отношения не имеют.
///
/// Список мостов берётся из того же слепка `OUT_DIR`, что и у сторожа
/// состава сборки: новый мост попадает под проверку автоматически.
#[cfg(test)]
mod qml_int64 {
    /// Мосты, ушедшие в `.files()` (тот же слепок, что у `build_manifest`).
    const BRIDGE_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bridge_files.list"));

    /// Типы, которые cxx-qt печатает 64-битным целым C++.
    const WIDE: &[&str] = &["i64", "u64", "i128", "u128", "isize", "usize"];

    /// ЕДИНСТВЕННОЕ исключение: транспортная обвязка запросов.
    ///
    /// `sendRequest` и `deliverResponse` — не интерфейс для QML, а шов между
    /// Rust и C++-транспортом (`cpp/rexon_we_transport.cpp`). Сигнал
    /// слушает C++, инвокабл зовёт `QMetaObject::invokeMethod` тем же
    /// метатипом — `Q_ARG(::std::int64_t, token)`. Из QML их не зовут
    /// нигде: `grep -rn "deliverResponse\|sendRequest"` по вендорному QML
    /// пуст. Номер кадра обязан оставаться точным целым, поэтому здесь
    /// `i64` уместен, а `f64` был бы ухудшением.
    ///
    /// ⚠️ Исключение действует по ИМЕНИ функции и только на эти два имени.
    /// Новое 64-битное объявление, как бы оно ни называлось, сторож
    /// назовёт поимённо и уронит `cargo test`.
    const ALLOWED_FNS: &[&str] = &["send_request", "deliver_response"];

    /// Одна найденная зацепка.
    struct Hit {
        file: String,
        line: usize,
        what: String,
    }

    /// Убрать комментарии: иначе сторож ловил бы собственные пояснения
    /// («`f64`, а не `i64`»). Строковых литералов с `//` в мостах нет.
    fn strip_comment(line: &str) -> &str {
        match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        }
    }

    /// Есть ли в куске текста 64-битный целый тип отдельным словом.
    fn has_wide(text: &str) -> bool {
        WIDE.iter().any(|ty| {
            text.match_indices(ty).any(|(at, _)| {
                let before = text[..at].chars().next_back();
                let after = text[at + ty.len()..].chars().next();
                let word = |c: Option<char>| {
                    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
                };
                !word(before) && !word(after)
            })
        })
    }

    /// Имя функции из объявления `fn name(...)`.
    fn fn_name(sig: &str) -> String {
        sig.split_once("fn ")
            .map(|(_, rest)| rest)
            .unwrap_or(sig)
            .split(['(', '<', ' '])
            .next()
            .unwrap_or("")
            .to_owned()
    }

    /// Разобрать один мост: вернуть все 64-битные объявления в блоках
    /// `extern "RustQt"`.
    fn scan(rel: &str, src: &str) -> Vec<Hit> {
        let lines: Vec<&str> = src.lines().collect();
        let mut hits = Vec::new();
        // Глубина фигурных скобок, на которой открылся блок RustQt.
        let mut depth: i32 = 0;
        let mut rust_qt_depth: Option<i32> = None;
        let mut index = 0;
        while index < lines.len() {
            let code = strip_comment(lines[index]);
            let trimmed = code.trim_start();

            let opens_rust_qt = trimmed.starts_with("extern \"RustQt\"")
                || trimmed.starts_with("unsafe extern \"RustQt\"");

            if rust_qt_depth.is_some() {
                // ── свойство ──────────────────────────────────────────────
                if trimmed.starts_with("#[qproperty(") {
                    let mut chunk = code.to_owned();
                    let mut last = index;
                    while chunk.matches('(').count() > chunk.matches(')').count()
                        && last + 1 < lines.len()
                    {
                        last += 1;
                        chunk.push(' ');
                        chunk.push_str(strip_comment(lines[last]).trim());
                    }
                    let ty = chunk
                        .split_once("#[qproperty(")
                        .map(|(_, rest)| rest)
                        .unwrap_or("")
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_owned();
                    if has_wide(&ty) {
                        let name = chunk
                            .split_once("#[qproperty(")
                            .map(|(_, rest)| rest)
                            .unwrap_or("")
                            .split(',')
                            .nth(1)
                            .unwrap_or("?")
                            .trim()
                            .to_owned();
                        hits.push(Hit {
                            file: rel.to_owned(),
                            line: index + 1,
                            what: format!("свойство `{name}` объявлено `{ty}`"),
                        });
                    }
                    index = last + 1;
                    continue;
                }
                // ── функция (инвокабл, сигнал, аксессор свойства) ─────────
                if trimmed.starts_with("fn ") || trimmed.starts_with("unsafe fn ") {
                    let mut chunk = code.to_owned();
                    let mut last = index;
                    while !chunk.contains(';')
                        && !chunk.trim_end().ends_with('{')
                        && last + 1 < lines.len()
                    {
                        last += 1;
                        chunk.push(' ');
                        chunk.push_str(strip_comment(lines[last]).trim());
                    }
                    let name = fn_name(&chunk);
                    if has_wide(&chunk) && !ALLOWED_FNS.contains(&name.as_str()) {
                        hits.push(Hit {
                            file: rel.to_owned(),
                            line: index + 1,
                            what: format!("подпись `{name}` несёт 64-битное целое"),
                        });
                    }
                    index = last + 1;
                    continue;
                }
            }

            depth += code.matches('{').count() as i32;
            depth -= code.matches('}').count() as i32;
            if opens_rust_qt && rust_qt_depth.is_none() {
                // Блок открылся: запомнить глубину ВНЕ него.
                rust_qt_depth = Some(depth - 1);
            } else if rust_qt_depth == Some(depth) {
                rust_qt_depth = None;
            }
            index += 1;
        }
        hits
    }

    #[test]
    fn no_wide_integers_reach_qml() {
        let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut hits = Vec::new();
        let mut checked = 0usize;
        for rel in BRIDGE_MANIFEST.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let path = crate_dir.join(rel);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("не читается {}: {e}", path.display()));
            checked += 1;
            hits.extend(scan(rel, &src));
        }
        assert!(
            checked > 0,
            "слепок мостов пуст — сторож ничего не проверил"
        );
        let report: Vec<String> = hits
            .iter()
            .map(|h| format!("{}:{} — {}", h.file, h.line, h.what))
            .collect();
        assert!(
            report.is_empty(),
            "64-битное целое торчит наружу в QML:\n  {}\n\n\
             Так делать нельзя: cxx-qt печатает `i64`/`u64` как `long` / \
             `unsigned long`, это отдельные метатипы Qt, и движок QML их не \
             знает. Литеральная декларативная привязка в такое свойство \
             («displayId: 7») ОТКАЗЫВАЕТ вместе со всем документом — \
             страница не создаётся вовсе, а ни сборка, ни остальные тесты \
             этого не видят.\n\
             Как чинить: объявить наружу `f64` (в QML — `double`), поле \
             внутри оставить точным, а на границе звать \
             `qvalue::i64_to_qml` / `u64_to_qml` и обратно \
             `qvalue::qml_to_i64` / `qml_to_u64`. Образец — \
             `src/playlist_item_select_storage.rs`.\n\
             ⚠️ Если значению НУЖНЫ все 64 бита (идентификатор Steam \
             Workshop, отметка времени в наносекундах), `double` не годится: \
             он держит целые точно лишь до 2^53. Такое значение выпускать \
             `QString`, а не числом.\n\
             Живая проверка после правки: QT_QPA_PLATFORM=offscreen \
             cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe",
            report.join("\n  ")
        );
    }
}
