// Сборка QML-модуля `org.rexon.gui`: cxx-qt прогоняет moc +
// qmltyperegistrar по мосту и запекает .qml в статический ресурс
// (qrc:/qt/qml/org/rexon/gui/). Статический плагин — потому что сборка
// чисто карговая, без CMake: динамический плагин требует раскладки qmldir
// рядом с бинарём (ограничение задокументировано в cxx-qt-build).
use cxx_qt_build::{CxxQtBuilder, QmlModule};

// Списки ниже — единственный источник правды о составе модуля. Они вынесены
// в константы не для красоты: их слепок уезжает в OUT_DIR, и сторож в тестах
// крейта (`src/lib.rs`, модуль `build_manifest`) сверяет с ним содержимое
// каталогов. Забытая строка ловится тестом, а не живым падением.
//
// ⚠️ Правку делать ЗДЕСЬ, а не в вызове ниже: вызов берёт эти константы.

/// Страницы и контролы, запекаемые в статический ресурс модуля.
const QML_FILES: &[&str] = &[
    "qml/LayerShellProbe.qml",
    // Живые модели в настоящем QML (сетка Discover, редактор свойств
    // обоев, гашение плашки) — сценарии `rexon-gui-selftest`.
    "qml/LiveModelsProbe.qml",
    "qml/WallpaperVideo.qml",
    // Веб-обои: тот же приём, тот же набор размещений, но
    // содержимое рисует QtWebEngine.
    "qml/WallpaperWeb.qml",
    // Клиент вывода демона waywallen на X11 (`--mode waywallen`): окна-
    // рабочие столы с элементом вывода waywallen.
    "qml/WallpaperWaywallen.qml",
    // Этап 3 ухода с PySide6: вкладки переписываются на QML по
    // одной. Первая — «Сравнение DE».
    "qml/ComparePage.qml",
    // Главное окно, у которого хозяин на Rust: `ApplicationWindow` вместо
    // `QMainWindow` продукта. Отдельный вход для разработчика (бинарь
    // `rexon-main-window`), живой Rexon его не грузит.
    "qml/MainWindow.qml",
    // Вкладка «Рабочие столы» — вкладка класса B на QML.
    "qml/DesktopsPage.qml",
    // Окно операции над столом — общее для «Рабочих столов» и
    // «Сравнения DE».
    "qml/DeOperationDialog.qml",
    // Вкладка «Настройки» — вторая вкладка класса B на QML.
    "qml/SettingsPage.qml",
    // Разделы «Анимация» и «Кнопки заголовка окна» той же вкладки —
    // отдельными компонентами со своими объектами действий.
    "qml/AnimationsSection.qml",
    // «Эффекты рабочего стола» (все эффекты KWin) — только в KDE.
    "qml/DesktopEffectsSection.qml",
    "qml/WindowButtonsSection.qml",
    // «Отклик при запуске» (как в модуле «Курсоры» KDE) — там же.
    "qml/LaunchFeedbackSection.qml",
    // «Цвет акцента» над сеткой «Цветовых схем» «Оформления».
    "qml/AccentSection.qml",
    // «Светлое и тёмное» (по расписанию) над «Глобальными темами» и «Цветовыми схемами».
    "qml/DayNightSection.qml",
    // «Сохранить текущее оформление…» над «Глобальными темами».
    "qml/GlobalSaveSection.qml",
    // Свой редактор цветовой схемы — когда редактора KDE нет.
    "qml/ColorSchemeEditor.qml",
    // «Значки и панели инструментов» (как «Параметры» стиля приложений KDE).
    "qml/AppWidgetsSection.qml",
    // «Управление шрифтами» внутри раздела шрифтов (действия — у самой
    // страницы, `SettingsActions`).
    "qml/FontManagerSection.qml",
    // Раздел «Окна Deepin»: где в Deepin меняются углы, прозрачность и
    // акцент окон — показывается только в сеансе Deepin.
    "qml/DdeLookSection.qml",
    // Раздел «Панель MATE»: готовые раскладки панели — только в сеансе MATE.
    // Самостоятельный компонент: потом переедет в «Оформление».
    "qml/MatePanelSection.qml",
    // Вкладка «Расширения» — вкладка класса B на QML.
    "qml/ExtensionsPage.qml",
    // Вкладка «Steam» — вкладка класса B на QML.
    "qml/SteamPage.qml",
    // Подвкладка «Витрины профиля» той же вкладки — отдельным компонентом
    // со своим объектом действий.
    "qml/SteamShowcaseSection.qml",
    // Вкладка «Оформление» — последняя вкладка класса B на QML.
    "qml/AppearancePage.qml",
    // Блок «Менеджер входа» в разделе экрана входа той же вкладки (данные —
    // проба `loginScreenReady`, действие — `makeDefaultLoginManager`).
    "qml/LoginManagerSection.qml",
    // Выбор файла у товара магазина с несколькими файлами — отдельным
    // компонентом ради стенда `LiveModelsProbe.qml` (сценарий «storefiles»).
    "qml/StoreFileChoice.qml",
    // Настройки виджета Conky в карточке раздела — отдельным компонентом:
    // положение, фон, окно, экран, часы и сеть (`conky_overlay`).
    "qml/ConkySettings.qml",
    // Наборы виджетов Conky над сеткой раздела (`conky_sets`).
    "qml/ConkySets.qml",
    // Вкладка «Wallpaper Engine» — шелл вокруг вендорного QML
    // waywallen: рейл и место страницы. Страницы вендорные и
    // приезжают отдельным модулем `waywallen.ui`, поэтому в
    // этот статический ресурс они не запекаются.
    "qml/WallpaperEnginePage.qml",
    // Скрытая страница «Разработчик» той же вкладки — НАША, а не
    // вендорная: обновления upstream и сверка паритета UI. Поэтому она
    // запекается сюда, в отличие от шести вендорных страниц.
    "qml/WeDeveloperPage.qml",
    // Контролы в скине приложения — общие для всех QML-вкладок:
    // стиль QtQuick Controls по умолчанию (Basic) палитре
    // приложения не следует, а менять стиль всему процессу
    // нельзя — рядом живёт вендорный QML вкладки Wallpaper Engine.
    "qml/RexonButton.qml",
    "qml/RexonDangerButton.qml",
    "qml/RexonCheckBox.qml",
    "qml/RexonComboBox.qml",
    "qml/RexonSlider.qml",
    "qml/RexonTextField.qml",
    // Всплывающая плашка состояния — одна на все вкладки: таймер гашения
    // живёт в ней, а не переписывается заново в каждой странице.
    "qml/RexonToast.qml",
    // Крупный просмотр картинки и вход в него с миниатюры — тоже одни на
    // все вкладки: лупа под курсором, щелчок, листание, масштаб.
    "qml/RexonImageViewer.qml",
    "qml/RexonZoomArea.qml",
    // Меню главного окна («☰») и диалоги за его пунктами: справка о
    // программе, вопрос перед сбросом темы KDE, окно удаления Rexon.
    "qml/WindowMenu.qml",
    "qml/WindowMenuDialogs.qml",
    // Обучение при первом запуске поверх окна на Rust.
    "qml/OnboardingOverlay.qml",
    // Мастер первого запуска: приветствие с выбором «для кого установить».
    // ⚠️ Накладка зовётся FirstRunWizardOverlay, а НЕ FirstRunWizard: имя
    // `FirstRunWizard` в модуле уже занял мост (`#[qml_element]` в
    // src/first_run_wizard.rs), и одноимённый файл сделал бы тип
    // двойственным («Ambiguous type detected … defined multiple times»), а
    // мост — недостижимым из разметки. Пара названа так же, как у обучения:
    // `OnboardingModel` + `OnboardingOverlay.qml`.
    "qml/FirstRunWizardOverlay.qml",
];

/// Мосты cxx-qt, которые прогоняются через moc и qmltyperegistrar.
///
/// Мост обязан лежать в одном каталоге с остальными #[qml_element]-мостами:
/// qmltyperegistrar берёт из moc только имя файла, без префикса пути
/// (QTBUG-93443), поэтому подкаталоги для мостов недопустимы.
const BRIDGE_FILES: &[&str] = &[
    // Хозяин главного окна на Rust: состав разделов и палитра.
    "src/main_window.rs",
    // Обучение при первом запуске: сценарий и прогресс из ядра для накладки.
    "src/onboarding_overlay.rs",
    // Мастер первого запуска: решение ядра, подписи, фоновая установка.
    // Разметка мастера — `qml/FirstRunWizardOverlay.qml` в списке выше.
    "src/first_run_wizard.rs",
    // Вкладка «Сравнение DE» — первая вкладка класса B на QML.
    "src/compare_page.rs",
    // Вкладка «Рабочие столы» — вкладка класса B на QML.
    "src/desktops_page.rs",
    // Объект побочных действий той же вкладки: версия пакета из
    // репозиториев и переход во встроенное «Сравнение DE». До него страница
    // получала `actions` только от Python.
    "src/desktops_actions.rs",
    // Вкладка «Настройки» — вторая вкладка класса B на QML.
    "src/settings_page.rs",
    // Объект побочных действий той же вкладки: снимки машины, скин,
    // автозапуск, файловый менеджер, родные приложения окружения, звуки
    // событий, защита приложений по умолчанию, импорт тем Windows. До него
    // страница получала `actions` от Python.
    "src/settings_actions.rs",
    // Разделы «Анимация» и «Кнопки заголовка окна»: снимок и запись — ядро
    // (`animations`, `window_buttons`).
    "src/animations_actions.rs",
    "src/app_widgets_actions.rs",
    "src/window_buttons_actions.rs",
    // «Отклик при запуске»: снимок и запись — ядро (`launch_feedback`).
    "src/launch_feedback_actions.rs",
    // «Цвет акцента»: снимок и запись — ядро (`accent`).
    "src/accent_actions.rs",
    // «Светлое и тёмное»: снимок, запись, таймер — ядро (`day_night`).
    "src/day_night_actions.rs",
    // «Сохранить текущее оформление…»: снимок, пакет, превью — ядро (`global_save`).
    "src/global_save_actions.rs",
    // Раздел «Окна Deepin»: снимок — ядро (`dde`).
    "src/dde_look_actions.rs",
    // Раздел «Панель MATE»: снимок, применение, возврат — ядро (`mate_panel`).
    "src/mate_panel_actions.rs",
    // Кнопки «послушать» тем звуков (ядро — `sound_preview`).
    "src/sound_preview_actions.rs",
    // Вкладка «Расширения» — вкладка класса B на QML.
    "src/extensions_page.rs",
    // Объект побочных действий той же вкладки: установка .rxn, удаление,
    // включение-выключение. До него страница получала `actions` от Python.
    "src/extensions_actions.rs",
    // Вкладка «Steam» — вкладка класса B на QML.
    "src/steam_page.rs",
    // Объект побочных действий той же вкладки: снимок витрины заставок,
    // выбор своего ролика, звёздочки, три переключателя, полноэкранный показ
    // и токен WebAPI. До него страница получала `actions` от Python.
    "src/steam_actions.rs",
    // Подвкладка «Витрины профиля»: нарезка картинки под витрину и фон для
    // предпросмотра (ядро — `steam_showcase`).
    "src/steam_showcase_actions.rs",
    // Вкладка «Оформление» — последняя вкладка класса B на QML.
    "src/appearance_page.rs",
    // Объект побочных действий той же вкладки: магазин, AUR, обзор диска,
    // картинки плиток и карусели, установка, применение и удаление тем.
    // До него страница получала `actions` от Python.
    "src/appearance_actions.rs",
    // Вкладка «Wallpaper Engine» — шелл вокруг вендорного QML.
    //
    // ⚠️ Забыть эту строку — значит получить мёртвую вкладку. Пока крейт
    // собирался ещё и как `cdylib`, сборка при этом оставалась ЗЕЛЁНОЙ
    // (неопределённые символы на линковке пропускались), и мост валился
    // только при `dlopen` живым Rexon («undefined symbol:
    // …WallpaperEngineModel…»). Наступили на это 30.08.2026; с тех пор
    // пропуск ловит сторож в тестах крейта.
    "src/we_page.rs",
    // Скрытая страница «Разработчик» вкладки обоев.
    "src/we_dev_page.rs",
    // Объект побочных действий той же вкладки: повторный запуск демона
    // обоев. До него страница получала `actions` от Python.
    "src/we_actions.rs",
    "src/discover_queries.rs",
    "src/display_queries.rs",
    "src/global_pause_queries.rs",
    "src/we_action.rs",
    "src/we_app.rs",
    "src/we_daemon_dbus.rs",
    "src/we_notify.rs",
    "src/we_objmodel.rs",
    "src/we_util.rs",
    "src/we_socket.rs",
    "src/layer_shell.rs",
    "src/wallpaper_host.rs",
    // `src/web_host.rs` мостом больше не является: QtWebEngineQuick
    // открывается `dlopen` только в веб-режиме хоста обоев (разбор там же).
    "src/wallpaper_audio.rs",
    "src/remote_list_model.rs",
    "src/tag_queries.rs",
    "src/thumbnail_request.rs",
    "src/source_queries.rs",
    "src/settings_queries.rs",
    "src/status_queries.rs",
    "src/remote_store_queries.rs",
    "src/plugin_queries.rs",
    // Подписи плагинов: каталог автора плагина поверх нашего русского слоя.
    "src/plugin_translations.rs",
    "src/playlist_queries.rs",
    // Выбор состава плейлиста: тип объявлен прямо в WallpaperPage.qml 0.3.7.
    "src/playlist_item_select_storage.rs",
    "src/autostart_queries.rs",
    "src/user_property_model.rs",
    "src/wallpaper_actions.rs",
    // Меню главного окна: состав пунктов, тексты диалогов и удаление Rexon.
    // Без этой строки мост не пройдёт через moc, и меню не появится в QML.
    "src/window_menu.rs",
    "src/wallpaper_list_model.rs",
    "src/wallpaper_queries.rs",
    "src/wallpaper_select_storage.rs",
];

/// Имена слепков в OUT_DIR. Те же строки читает сторож в `src/lib.rs`.
const QML_MANIFEST: &str = "qml_files.list";
const BRIDGE_MANIFEST: &str = "bridge_files.list";

/// Положить в OUT_DIR ровно то, что уходит в сборщик.
///
/// ⚠️ Сверять состав каталогов прямо здесь бессмысленно: при добавлении
/// НОВОГО файла build.rs может вообще не перезапуститься — а это ровно тот
/// случай, который надо поймать. Поэтому сверку делает тест, который
/// выполняется всегда, а build.rs только честно отчитывается о своём списке.
fn write_manifest(name: &str, files: &[&str]) {
    let out_dir = std::env::var("OUT_DIR").expect("cargo обязан задать OUT_DIR");
    let path = std::path::Path::new(&out_dir).join(name);
    let body = files.join("\n");
    // Писать только при изменении: лишняя перезапись файла в OUT_DIR
    // заставила бы cargo считать сборку грязной.
    if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
        std::fs::write(&path, body).expect("не удалось записать слепок состава сборки");
    }
}

fn main() {
    write_manifest(QML_MANIFEST, QML_FILES);
    write_manifest(BRIDGE_MANIFEST, BRIDGE_FILES);
    build_translations();

    // Собирать против того же Qt6, что использует PySide6, — на время
    // strangler-миграции Rust-модели живут в одном процессе с Python, ABI
    // обязан совпадать. Без подсказки cxx-qt может взять qmake от Qt5,
    // который в системе тоже есть (та же причина, что в rexon-qt-filter).
    if std::env::var_os("QMAKE").is_none() {
        for cand in ["/usr/bin/qmake6", "/usr/lib/qt6/bin/qmake"] {
            if std::path::Path::new(cand).exists() {
                std::env::set_var("QMAKE", cand);
                break;
            }
        }
    }

    // LayerShellQt — библиотека KDE (пакет layer-shell-qt), привязок к Rust у
    // неё нет, поэтому она подключается как обычная C++-зависимость шима
    // cpp/rexon_layer_shell.cpp. Заголовки лежат в системном include-дереве
    // (/usr/include/LayerShellQt/), отдельный .pc-файл она не ставит — только
    // конфиг CMake, который карго-сборке не поможет.
    //
    // Проверка наличия здесь, а не «когда-нибудь на линковке»: без неё
    // отсутствие пакета вылезло бы простынёй ошибок компилятора.
    let layer_shell_header = ["/usr/include/LayerShellQt/window.h"]
        .iter()
        .find(|p| std::path::Path::new(p).exists());
    assert!(
        layer_shell_header.is_some(),
        "не найден заголовок LayerShellQt (/usr/include/LayerShellQt/window.h). \
         Слой обоев без него собрать нельзя — поставьте пакет layer-shell-qt"
    );
    println!("cargo:rustc-link-lib=LayerShellQtInterface");

    // SAFETY (`cc_builder`): колбэк только ДОБАВЛЯЕТ каталоги заголовков Qt и не
    // трогает флаги и пути, которые выставляет сам cxx-qt-build.
    unsafe {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.rexon.gui")
            .version(1, 0)
            .qml_files(QML_FILES.iter().copied()),
    )
    .qt_module("Quick")
    // Значок в лотке: QSystemTrayIcon, QMenu, QAction и поиск главного окна
    // живут в QtWidgets. Новой зависимости у ПРОДУКТА от этого не появляется
    // — процесс Rexon и так QApplication из PySide6, то есть libQt6Widgets в
    // нём уже загружена; здесь мы только перестаём делать вид, что её нет.
    .qt_module("Widgets")
    .qt_module("DBus")
    .qt_module("Network")
    // Партия 7: транспорт вкладки WE (QWebSocket) переехал сюда из
    // client.py — без этого модуля сокет не линкуется.
    .qt_module("WebSockets")
    // QtWebEngineQuick здесь НЕ линкуется: хост обоев открывает её `dlopen`
    // только в веб-режиме (src/web_host.rs). Прямая ссылка тянула бы
    // Chromium в каждый старт видео-обоев того же бинаря.
    // Поток кадров видео-обоев берёт кадр из QVideoSink (cpp/rexon_wallpaper_host.cpp):
    // на платформе offscreen VideoOutput не рисует, снимок окна чёрный.
    // Новой зависимости у продукта нет — QML видео-обоев и так грузит QtMultimedia.
    .qt_module("Multimedia")
    // Образцы тем на плитках «Оформления» рисуются из рисунков самих тем
    // (рамки стилей Plasma и Aurorae — части SVG по именам):
    // cpp/rexon_local_preview.cpp зовёт QSvgRenderer. Пакет qt6-svg и так в
    // зависимостях: им Qt читает значки SVG.
    .qt_module("Svg")
    .include_dir("cpp")
    // Логотип и журнал выпусков waywallen по адресу их разметки
    // (`qrc:/waywallen/ui/assets/…`): без этого их страницы ищут файлы в
    // ресурсах чужого бинаря и не находят. Разбор — в самом .qrc.
    .qrc("waywallen_assets.qrc")
    // Flash-режим хоста обоев берёт wl_surface окна через
    // `qpa/qplatformnativeinterface.h` (cpp/rexon_wallpaper_host.cpp): этот
    // заголовок лежит в каталогах с номером версии Qt, которых нет в
    // публичных путях cxx-qt-build.
    .cc_builder(|cc| {
        for dir in qt_private_include_dirs() {
            cc.include(dir);
        }
    })
    // Гаджетный шим — ОДИН на проект: исходник живёт в
    // rexon-qt-filter, куда его положила модель редактора фильтров, а сюда
    // подключается тем же файлом. Второй копии шима в проекте нет.
    .include_dir("../rexon-qt-filter/cpp")
    .cpp_files([
        "cpp/rexon_gui_factory.cpp",
        "cpp/rexon_layer_shell.cpp",
        // Хост видео-обоев: идентификатор окна X11 и поток кадров в mmap —
        // до обоих из Rust не дотянуться (нет QImage в cxx-qt-lib 0.9).
        "cpp/rexon_wallpaper_host.cpp",
        // Плейсхолдер превью локальной темы: QPainter поверх QPixmap,
        // drawText с прямоугольником и QFontMetrics — ничего из этого в
        // cxx-qt-lib 0.9 нет. План операций строит ядро, исполняет
        // src/local_preview_render.rs.
        "cpp/rexon_local_preview.cpp",
        // Снимок формы из настоящих виджетов в стиле Qt (помощник
        // rexon-theme-shot): QApplication, QStyleFactory, QWidget, grab() —
        // виджетов в cxx-qt-lib нет.
        "cpp/rexon_theme_shot.cpp",
        // Регистрация типов в `waywallen.ui` для живого Rexon: подключается
        // здесь, потому что имя сгенерированного cxx-qt заголовка известно
        // только после обработки мостов ниже.
        "cpp/rexon_register_we.cpp",
        // Значок в системном лотке: QSystemTrayIcon + QMenu + перехват
        // закрытия главного окна. Ни того, ни другого в cxx-qt-lib 0.9 нет.
        "cpp/rexon_tray.cpp",
        // Мост «не гасить экран»: служба на шине — наследник
        // QDBusVirtualObject, а QtDBus в cxx-qt-lib нет. Решения — в ядре
        // (rexon_core::screensaver_bridge).
        "cpp/rexon_screensaver_bridge.cpp",
        // Диалоги вкладки «Расширения»: выбор .rxn и вопрос «да/нет».
        // Виджетных QFileDialog/QMessageBox в QML-процессе нет, а QML-типы
        // асинхронны — разбор решения в шапке заголовка.
        "cpp/rexon_ext_dialogs.cpp",
        // Обратный адрес запросов: номер → QPointer на запрос и вызов его
        // `deliverResponse` через метаобъект (причина C++ — в шапке .h).
        "cpp/rexon_we_transport.cpp",
        // Партия 7: сам транспорт — то, чего нет в cxx-qt-lib вовсе
        // (создание QObject, QtDBus, QTimer, QProcess, вложенный QEventLoop).
        "cpp/rexon_we_socket.cpp",
        // Партия 5: Qt-услуги синглтонов (D-Bus, сетевой кэш, буфер обмена)
        // и живые модели App.
        "cpp/rexon_we_singletons.cpp",
        "cpp/rexon_we_objmodel.cpp",
        // База синглтона Action: сигнал с дефолтными аргументами. Заголовок,
        // а не .cpp, — cxx-qt-build прогоняет через moc именно заголовки.
        "cpp/rexon_we_action_base.h",
        "../rexon-qt-filter/cpp/rexon_gadget_shim.cpp",
    ])
    .files(BRIDGE_FILES.iter().copied())
    .build();
    }
}

/// Каталоги приватных заголовков Qt с номером версии
/// (`/usr/include/qt6/QtGui/6.x.y` и `…/QtGui`, то же для QtCore). Версию
/// берём по самому каталогу, а не из qmake: в дереве заголовков он один.
fn qt_private_include_dirs() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new("/usr/include/qt6");
    let mut dirs = Vec::new();
    for module in ["QtCore", "QtGui"] {
        let Ok(entries) = std::fs::read_dir(root.join(module)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && name.starts_with(|c: char| c.is_ascii_digit()) {
                dirs.push(entry.path());
                dirs.push(entry.path().join(module));
            }
        }
    }
    dirs
}

/// Каталоги перевода интерфейса (`po/*.po` → `.mo`; его читают и Rust, и QML)
/// рядом с собранными бинарями: `target/<профиль>/locale/<язык>/LC_MESSAGES/`.
///
/// Именно там их ищет `rexon_core::i18n` у бинаря из дерева разработки, а
/// пакет переносит тот же каталог в `/usr/share/locale`. Сборка здесь, а не
/// отдельной командой, — чтобы `cargo build-app` не мог дать свежий бинарь
/// со старыми переводами: забытый шаг значил бы английский интерфейс у
/// русского покупателя без единой ошибки.
///
/// Сама сборка — в `scripts/i18n-update.sh --build` (одна дверь для
/// сборки, пакета и человека). Нет программ перевода — сборка встаёт с
/// объяснением, а не молча выпускает продукт без языков.
fn build_translations() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..");
    let po = root.join("po");
    let script = root.join("scripts/i18n-update.sh");
    println!("cargo:rerun-if-changed={}", po.display());
    println!("cargo:rerun-if-changed={}", script.display());
    if !po.join("LINGUAS").is_file() {
        return;
    }
    // OUT_DIR = target/<профиль>/build/<крейт>-<хэш>/out: профиль — на три
    // уровня выше (с `--target` путь длиннее спереди, но не сзади).
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR глубже трёх уровней")
        .to_path_buf();
    let dest = profile_dir.join("locale");
    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg("--build")
        .arg(&dest)
        .status()
        .expect("bash запускается");
    assert!(
        status.success(),
        "каталоги перевода не собрались ({}): нужен msgfmt (пакет gettext); \
         подробности — выше в выводе сборки",
        script.display()
    );
}
