// App — корневой QML-синглтон вкладки Wallpaper Engine.
//
// Порт
// `ui/src/app.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//
// Что делает: держит живые модели (`rendererManager`/`displayManager`/
// `libraryManager`/`gpuManager`/`presentationManager`), кормит их событиями демона и заведует сетевым кэшем
// QML-картинок, который у вендорной страницы настроек есть отдельным
// разделом.
//
// ⚠️ Четыре члена, которых в Python-мосте НЕ БЫЛО ВОВСЕ, хотя вендорный QML
// их зовёт: `networkCacheSize`, `networkCacheMaximumSize`,
// `refreshNetworkCacheSize()` и `clearNetworkCache()`
// (`SettingsPage.qml:124/132/768/776/778`). Раздел «Сетевой кэш картинок»
// показывал «NaN из NaN», а кнопка «Очистить» падала в TypeError. Кэш при
// этом существовал — его ставил `host.py` своей фабрикой, — просто им никто
// не управлял. Теперь кэш один и живёт в шиме
// (`cpp/rexon_we_singletons.cpp`), а host.py только показывает ему движок:
// две копии настройки кэша разъехались бы молча (правило #12).
//
// Видеокарты (`gpuManager`) — единственная модель, которую демон событиями
// не кормит: карты он перечисляет один раз при старте. Поэтому App сам
// спрашивает `gpu_list` — при создании, если сокет уже открыт, и на каждом
// подключении (у оригинала это `GpuListQuery::reload` в `App::init`). Ответ
// возвращается сюда же по номеру запроса (`deliverResponse`).

/// Мост cxx-qt: синглтон `App`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "C++" {
        include!("rexon_we_objmodel.h");
        /// Живые модели — дети App, как `Box<...>` в `AppPrivate` оригинала.
        ///
        /// # Safety
        /// `parent` — живой QObject (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_renderer_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_display_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_library_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_gpu_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_presentation_manager(parent: *mut QObject) -> *mut QObject;

        include!("rexon_we_singletons.h");
        /// Записаться получателем кадров демона.
        ///
        /// # Safety
        /// `app` — живой QObject (указатель на самого себя).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_register_app(app: *mut QObject);

        /// Язык интерфейса вендорных страниц. Какой язык — решает Rexon
        /// (`rexon_core::i18n`, настройка `ui_language`), какой каталог
        /// грузить — ядро (`rexon_core::remote_l10n`); здесь только Qt:
        /// QTranslator и пересборка привязок движка.
        fn rexon_we_apply_ui_language(
            locale_name: &QString,
            qm_path: &QString,
            required: bool,
        ) -> QString;

        include!("rexon_register_we.h");
        /// Язык подписей плагинов — вслед за языком интерфейса, как в
        /// `App::setUiLanguage` оригинала.
        fn rexon_we_set_plugin_locale(locale: &QString);

        fn rexon_we_network_cache_size() -> i64;
        fn rexon_we_network_cache_max() -> i64;
        fn rexon_we_set_network_cache_max(bytes: i64);
        fn rexon_we_clear_network_cache();
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(*mut QObject, renderer_manager, cxx_name = "rendererManager",
                    READ = renderer_manager, CONSTANT)]
        #[qproperty(*mut QObject, display_manager, cxx_name = "displayManager",
                    READ = display_manager, CONSTANT)]
        #[qproperty(*mut QObject, library_manager, cxx_name = "libraryManager",
                    READ = library_manager, CONSTANT)]
        /// Сам менеджер один на всю жизнь App. Оповещение у свойства —
        /// наше, у оригинала оно постоянное: значок карты (`GpuTag.qml`)
        /// ищет карту вызовом `find` внутри привязки, которая читает это
        /// свойство, и без сигнала не узнал бы, что список сменился, —
        /// показывал бы пустую плашку до пересоздания страницы.
        #[qproperty(*mut QObject, gpu_manager, cxx_name = "gpuManager",
                    READ = gpu_manager, NOTIFY = gpu_manager_changed)]
        /// ⚠️ Оба размера уходят наружу `f64`, а не `i64`: 64-битное целое в
        /// QML не выпускаем (разбор — `src/qvalue.rs`). Байты кэша до
        /// потолка `double` (2^53 = 9 ПБ) не дотягиваются даже близко.
        /// Выбор языка интерфейса — ОБЩИЙ с Rexon (`ui_language`):
        /// `system`, `en` или код каталога Rexon (`ru`).
        #[qproperty(QString, ui_language, cxx_name = "uiLanguage",
                    READ = ui_language, NOTIFY = ui_language_changed)]
        /// Список для комбобокса «Язык»: карты `{code, label}` — тот же
        /// список, что в «Настройках» Rexon (`qt_settings::language_choices`):
        /// «Как в системе», English, дальше языки Rexon самоназваниями.
        /// Оповещается тем же сигналом, что и выбор
        /// (`availableUiLanguages NOTIFY uiLanguageChanged` оригинала).
        #[qproperty(QList_QVariant, available_ui_languages,
                    cxx_name = "availableUiLanguages",
                    READ = available_ui_languages, NOTIFY = ui_language_changed)]
        #[qproperty(f64, network_cache_size, cxx_name = "networkCacheSize",
                    READ = network_cache_size, NOTIFY = network_cache_size_changed)]
        #[qproperty(f64, network_cache_maximum_size, cxx_name = "networkCacheMaximumSize",
                    READ = network_cache_maximum_size,
                    NOTIFY = network_cache_maximum_size_changed)]
        /// Текущие показы обоев (`PresentationManager`): по нему рейка
        /// вкладки рисует карточку «Текущие обои» (`qml_ui/PageHost.qml`).
        #[qproperty(*mut QObject, presentation_manager, cxx_name = "presentationManager",
                    READ = presentation_manager, CONSTANT)]
        /// Версия ВЕНДОРНОГО интерфейса (`0.5.0`) — тег `ui/UPSTREAM_TAG`,
        /// разбор в `rexon_core::qml_modules::ui_version`. Своего члена у
        /// оригинала нет: там строку «Версия %1» на «О программе» пишет
        /// `Qt.application.version`, а у нас это версия Rexon (патч 0002).
        #[qproperty(QString, ui_version, cxx_name = "uiVersion",
                    READ = ui_version, CONSTANT)]
        /// Своего члена у оригинала нет (патч 0010): «Применить» на
        /// карточке обоев доступна и при нуле дисплеев там, где дисплей
        /// поднимает Rexon (KDE — плагин, X11 — хост-клиент вывода): кадр
        /// придержится, вывод поднимется, обои лягут на все столы
        /// (`waywallen_kde_plugin::apply_without_displays`).
        #[qproperty(bool, apply_without_displays, cxx_name = "applyWithoutDisplays",
                    READ = apply_without_displays, CONSTANT)]
        /// Своего члена у оригинала нет (патч 0010): запуск плейлиста при
        /// нуле дисплеев — цель «все рабочие столы», кадр придержится,
        /// вывод поднимется (`waywallen_kde_plugin::play_without_displays`).
        #[qproperty(bool, play_without_displays, cxx_name = "playWithoutDisplays",
                    READ = play_without_displays, CONSTANT)]
        /// Подсказка страницы «Мониторы» при нуле дисплеев вместо вендорной
        /// «поставьте расширение руками» (KDE) или голого «Мониторы не
        /// зарегистрированы» (X11) — патч 0010; пусто — не наш случай.
        #[qproperty(QString, no_displays_hint, cxx_name = "noDisplaysHint",
                    READ = no_displays_hint, CONSTANT)]
        type App = super::AppRust;
    }

    unsafe extern "RustQt" {
        /// Ошибка транспорта → тост окна (v0.2.6, `Window.qml
        /// onErrorOccurred`, у нас — `PageHost.qml`).
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut App>, error: QString);

        /// Информационное сообщение транспорта (не ошибка): тост, который
        /// гаснет сам через `duration_ms` (`ui_base::toast_timeout_for_text`).
        /// Сигнал Rexon, у вендора его нет; слушает `PageHost.qml`.
        #[qsignal]
        #[cxx_name = "noticeOccurred"]
        fn notice_occurred(self: Pin<&mut App>, text: QString, duration_ms: i32);

        #[qsignal]
        #[cxx_name = "uiLanguageChanged"]
        fn ui_language_changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "gpuManagerChanged"]
        fn gpu_manager_changed(self: Pin<&mut App>);

        #[qsignal]
        #[cxx_name = "networkCacheSizeChanged"]
        fn network_cache_size_changed(self: Pin<&mut App>);

        #[qsignal]
        #[cxx_name = "networkCacheMaximumSizeChanged"]
        fn network_cache_maximum_size_changed(self: Pin<&mut App>);
    }

    unsafe extern "RustQt" {
        #[cxx_name = "rendererManager"]
        fn renderer_manager(self: &App) -> *mut QObject;
        #[cxx_name = "displayManager"]
        fn display_manager(self: &App) -> *mut QObject;
        #[cxx_name = "libraryManager"]
        fn library_manager(self: &App) -> *mut QObject;
        #[cxx_name = "gpuManager"]
        fn gpu_manager(self: &App) -> *mut QObject;
        #[cxx_name = "presentationManager"]
        fn presentation_manager(self: &App) -> *mut QObject;
    }

    extern "RustQt" {
        #[cxx_name = "uiLanguage"]
        fn ui_language(self: &App) -> QString;
        #[cxx_name = "uiVersion"]
        fn ui_version(self: &App) -> QString;
        #[cxx_name = "applyWithoutDisplays"]
        fn apply_without_displays(self: &App) -> bool;
        #[cxx_name = "playWithoutDisplays"]
        fn play_without_displays(self: &App) -> bool;
        #[cxx_name = "noDisplaysHint"]
        fn no_displays_hint(self: &App) -> QString;
        #[cxx_name = "availableUiLanguages"]
        fn available_ui_languages(self: &App) -> QList_QVariant;

        #[cxx_name = "networkCacheSize"]
        fn network_cache_size(self: &App) -> f64;
        #[cxx_name = "networkCacheMaximumSize"]
        fn network_cache_maximum_size(self: &App) -> f64;

        /// Пересчитать размер кэша (страница настроек зовёт при открытии).
        #[qinvokable]
        #[cxx_name = "refreshNetworkCacheSize"]
        fn refresh_network_cache_size(self: Pin<&mut App>);

        /// Порт `App::setNetworkCacheMaximumSize`: неположительное значение
        /// и повтор игнорируются.
        #[qinvokable]
        #[cxx_name = "setNetworkCacheMaximumSize"]
        fn set_network_cache_maximum_size(self: Pin<&mut App>, size: f64);

        #[qinvokable]
        #[cxx_name = "clearNetworkCache"]
        fn clear_network_cache(self: Pin<&mut App>);

        /// Сменить язык интерфейса (порт `App::setUiLanguage`). Выбор
        /// записывается в настройку Rexon `ui_language` — ту же, что меняют
        /// «Настройки», — вендорные страницы переводятся сразу, остальной
        /// Rexon после перезапуска. `false` — язык не поддерживается или
        /// каталог не загрузился; вендорный QML показывает тост
        /// «Не удалось изменить язык».
        #[qinvokable]
        #[cxx_name = "setUiLanguage"]
        fn set_ui_language(self: Pin<&mut App>, language: &QString) -> bool;

        /// Переключатель «одно окно интерфейса» со страницы настроек
        /// (вендор 0.3.7).
        ///
        /// ⚠️ Осознанная развилка Rexon. Оригинал занимает на шине имя
        /// `org.waywallen.waywallen.UI` и экспортирует метод `Raise`:
        /// второй запуск ЕГО интерфейса не открывает окно, а поднимает
        /// уже открытое. У нас интерфейс — вкладка внутри Rexon, и
        /// занять это имя означало бы отобрать его у настоящего окна
        /// waywallen: тот при запуске увидел бы занятое имя, позвал бы
        /// `Raise` в никуда и молча закрылся, так и не открывшись.
        /// Своё единственное окно Rexon держит сам, поэтому переключатель
        /// только помнит выбор (`Global.qml` хранит его в настройках), а
        /// имя на шине не занимается.
        #[qinvokable]
        #[cxx_name = "setSingleUiEnabled"]
        fn set_single_ui_enabled(self: Pin<&mut App>, enabled: bool);

        /// Сырой кадр демона от шва (`rexon_we_do_deliver_event`) — раздать
        /// живым моделям. Имя обязано совпадать с тем, что ищет
        /// `QMetaObject::invokeMethod`.
        #[qinvokable]
        #[cxx_name = "deliverEvent"]
        fn deliver_event(self: Pin<&mut App>, frame: &QByteArray);

        /// Ошибка питоновского клиента → сигнал `errorOccurred`.
        #[qinvokable]
        #[cxx_name = "clientError"]
        fn client_error(self: Pin<&mut App>, error: &QString);

        /// Полная замена списка рендереров ответом `renderer_list`.
        ///
        /// На вход СЫРОЙ кадр демона — тот же, что разобрал сам запрос
        /// (`status_queries::RendererListQuery`). У оригинала эту
        /// синхронизацию делает сам запрос; у нас он в другом мосте, и
        /// добраться до живых моделей `App` можно только через шов.
        #[qinvokable]
        #[cxx_name = "replaceRenderers"]
        fn replace_renderers(self: Pin<&mut App>, frame: &QByteArray);

        /// Ответ демона на запрос, отправленный самим App (`gpu_list`).
        /// Зовёт транспортный шов по номеру запроса; имя обязано совпадать
        /// с тем, что ищет `QMetaObject::invokeMethod`.
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut App>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Initialize for App {}
    impl cxx_qt::Threading for App {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use prost::Message;

use rexon_core::proto::control as pb;
use rexon_core::remote_l10n as l10n;

use crate::we_objmodel::qobject::{
    DisplayManager, GpuManager, LibraryManager, PresentationManager, RendererManager,
};
use crate::we_socket::{self, DaemonEvent};

/// Указатели на модели сырые: владеет ими Qt (они дети App и умрут вместе
/// с ним) — та же схема, что у владеемых моделей запросов.
pub struct AppRust {
    renderer_manager: *mut qobject::QObject,
    display_manager: *mut qobject::QObject,
    library_manager: *mut qobject::QObject,
    gpu_manager: *mut qobject::QObject,
    /// Номер ждущего запроса `gpu_list` (`0` — не ждём). Ответ с другим
    /// номером — от прошлого подключения, его не берём.
    gpu_request: i64,
    presentation_manager: *mut qobject::QObject,
    /// Номер ждущего запроса `wallpaper_lookup` для карточки текущих обоев
    /// (`0` — не ждём). Новый снимок показов заводит новый запрос, и ответ
    /// на прежний уже не берётся.
    presentation_request: i64,
    /// Подписка на события транспорта: по ней ловим подключение и обрыв.
    subscription: u64,
    /// Кэш размера: сигнал шлётся только при РЕАЛЬНОЙ смене (ветка
    /// `App::refreshNetworkCacheSize` оригинала).
    network_cache_size: i64,
    /// Выбор языка интерфейса (`system`, `en`, `ru`) — настройка Rexon
    /// `ui_language`, а не своя.
    ui_language: String,
    /// Языки, у которых рядом с модулем есть собранный каталог `.qm`.
    /// Считается один раз: каталоги приезжают вместе со сборкой и за время
    /// работы не появляются.
    ui_language_codes: Vec<String>,
    /// Каталог, где лежат эти `.qm` (рядом с модулем UI).
    ui_module_dir: String,
    /// Версия вендорного интерфейса (`uiVersion`): читается один раз — тег
    /// приезжает вместе с разметкой и за время работы не меняется.
    ui_version: String,
}

impl Default for AppRust {
    fn default() -> Self {
        Self {
            renderer_manager: std::ptr::null_mut(),
            display_manager: std::ptr::null_mut(),
            library_manager: std::ptr::null_mut(),
            gpu_manager: std::ptr::null_mut(),
            gpu_request: 0,
            presentation_manager: std::ptr::null_mut(),
            presentation_request: 0,
            subscription: 0,
            network_cache_size: 0,
            ui_language: l10n::UI_LANGUAGE_SYSTEM.to_string(),
            ui_language_codes: Vec::new(),
            ui_module_dir: String::new(),
            ui_version: rexon_core::qml_modules::UPSTREAM_TAG_UNKNOWN.to_string(),
        }
    }
}

impl Drop for AppRust {
    fn drop(&mut self) {
        we_socket::unsubscribe(self.subscription);
    }
}

impl cxx_qt::Initialize for qobject::App {
    fn initialize(mut self: Pin<&mut Self>) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::App).cast()
        };
        // Модели создаются в пост-конструкции, а не лениво в геттере: QML
        // читает свойство при первой же привязке, а геттер по контракту
        // константный. Порядок как у `AppPrivate` оригинала.
        let renderers = unsafe { qobject::rexon_new_renderer_manager(parent) };
        let displays = unsafe { qobject::rexon_new_display_manager(parent) };
        let libraries = unsafe { qobject::rexon_new_library_manager(parent) };
        let gpus = unsafe { qobject::rexon_new_gpu_manager(parent) };
        let presentations = unsafe { qobject::rexon_new_presentation_manager(parent) };
        if let Some(manager) = unsafe { pin_as::<PresentationManager>(presentations) } {
            manager.attach();
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.renderer_manager = renderers;
            rust.display_manager = displays;
            rust.library_manager = libraries;
            rust.gpu_manager = gpus;
            rust.presentation_manager = presentations;
        }
        // Язык вендорных страниц — ДО первой загрузки QML: переводчик
        // должен стоять к моменту, когда движок компилирует страницу.
        //
        // ⭐ Язык — тот же, на котором говорит весь Rexon в этом процессе
        // (`i18n::current_language`). Раньше у вкладки был свой выбор в
        // `QSettings` (`ui/language`) и свой «Системный» по `QLocale`: при
        // английском Rexon на русской системе вкладка обоев оставалась
        // русской, и наоборот. Прежний ключ `QSettings` больше не читается.
        let module_dir = l10n::ui_module_dir(&crate::we_env::pkg_dir());
        let codes = discover_ui_languages(&module_dir);
        let spoken = rexon_core::i18n::current_language();
        apply_ui_language(&codes, &module_dir, &spoken);
        let tag_path = rexon_core::qml_modules::qml_path(
            &module_dir,
            rexon_core::qml_modules::UPSTREAM_TAG_REL,
        );
        let ui_version = rexon_core::qml_modules::ui_version(
            std::fs::read_to_string(&tag_path).ok().as_deref(),
        );
        {
            let mut rust = self.as_mut().rust_mut();
            rust.ui_version = ui_version;
            rust.ui_module_dir = module_dir;
            rust.ui_language_codes = codes;
            rust.ui_language = rexon_core::i18n::saved_setting();
        }

        // Получателем кадров — сразу: демон шлёт снимки дисплеев и
        // рендереров один раз на WS-connect.
        unsafe { qobject::rexon_we_register_app(parent) };

        // Видеокарты: спросить сразу (App мог родиться при уже открытом
        // сокете — сигнала подключения тогда не будет) и на каждом
        // следующем подключении.
        let thread = self.qt_thread();
        let subscription = we_socket::subscribe(move |event| match event {
            DaemonEvent::Connected => {
                // Объект мог умереть — это не повод падать.
                let _ = thread.queue(|app| app.request_gpu_list());
            }
            // Обрыв связи: показов нет, пока демон не пришлёт новый снимок
            // (у оригинала список чистит `Backend::disconnected`).
            DaemonEvent::Disconnected { .. } => {
                let _ = thread.queue(|app| app.presentations_disconnected());
            }
            // Сообщение транспорта человеку (например, «живые обои не
            // запустились — прежние обои стола возвращены») — ИНФОРМАЦИОННЫМ
            // тостом, который гаснет сам (`PageHost.qml`, `onNoticeOccurred`).
            // ⛔ Прежде шло тостом ошибки: тот без срока, и плашка висела
            // больше минуты поверх кнопок панели (ВМ, r1895, 30.10.2026).
            DaemonEvent::Notice(text) => {
                let text = text.clone();
                let ms = rexon_core::ui_base::toast_timeout_for_text(&text);
                let ms = i32::try_from(ms).unwrap_or(i32::MAX);
                let _ = thread.queue(move |app| app.notice_occurred(QString::from(&text), ms));
            }
            _ => {}
        });
        self.as_mut().rust_mut().subscription = subscription;
        self.request_gpu_list();
    }
}

/// Языки собранных каталогов рядом с модулем UI.
///
/// Оригинал берёт их из ресурса `:/i18n`; у нас `.qm` лежат файлами рядом с
/// `remote_l10n.py` — читаем каталог, а отбор и порядок оставляем ядру.
fn discover_ui_languages(module_dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(module_dir) else {
        rexon_core::log_warn!(
            "lang",
            "каталоги переводов вкладки обоев не прочитаны ({module_dir}) — в списке языков останутся «Системный» и English"
        );
        return Vec::new();
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    l10n::ui_language_codes(&names)
}

/// Язык вендорного каталога для языка Rexon `code` (`ru`, `pt_BR`, `en`):
/// свой каталог (точно или по базовому языку), иначе английский — строки
/// вендора и так английские. «Системного» здесь не бывает: систему уже
/// разобрал Rexon.
fn vendor_language(code: &str, codes: &[String]) -> String {
    l10n::normalize_ui_language(code, codes)
        .filter(|c| c != l10n::UI_LANGUAGE_SYSTEM)
        .unwrap_or_else(|| l10n::UI_LANGUAGE_ENGLISH.to_string())
}

/// Поставить вкладке обоев язык Rexon `rexon_language` (`ru`, `de`, `en`,
/// `pt_BR`); `None` — не вышло.
///
/// Не «системный»: какой язык у системы, решает Rexon, а не `QLocale`
/// вендора. Решений здесь два, и они РАЗНЫЕ:
///   * каталог вендорных страниц — свой каталог языка Rexon, а нет его —
///     исходные (английские) строки ([`vendor_language`]);
///   * подписи плагинов — язык Rexon как есть: у плагинов свои каталоги
///     (`i18n/ru.po`, `zh-CN.po`), и есть ли каталог у вендорных страниц, им
///     безразлично. Раньше сюда уходил выбор вендорного каталога, и при
///     языке, у которого страниц нет, а плагин переведён, подписи плагина
///     оставались английскими.
fn apply_ui_language(codes: &[String], module_dir: &str, rexon_language: &str) -> Option<String> {
    let preference = vendor_language(rexon_language, codes);
    let plan = l10n::plan_ui_language(&preference, "", codes)?;
    let qm_path = if plan.qm_file.is_empty() {
        String::new()
    } else {
        std::path::Path::new(module_dir)
            .join(&plan.qm_file)
            .to_string_lossy()
            .into_owned()
    };
    let resolved = qobject::rexon_we_apply_ui_language(
        &QString::from(&plan.locale_name),
        &QString::from(&qm_path),
        plan.required,
    );
    if resolved.to_string().is_empty() {
        return None;
    }
    // Подписи плагинов идут за языком Rexon (см. выше), а не за каталогом
    // страниц; английский — тот, что разрешил Qt.
    let plugin_locale = if rexon_language.trim().is_empty() {
        resolved.to_string()
    } else {
        rexon_language.to_string()
    };
    qobject::rexon_we_set_plugin_locale(&QString::from(&plugin_locale));
    // Строка того же назначения, что `applyLanguage` оригинала: без неё
    // «страницы вдруг по-английски» неотличимо от «каталог не собрался» и
    // от «каталог не нашли». ⚠️ Язык Rexon и язык страниц пишутся ОБА:
    // прежняя строка «выбор=en» при немецком Rexon читалась как «вкладка
    // выбрала английский вместо немецкого», хотя выбирать было не из чего.
    rexon_core::log_info!(
        "lang",
        "язык вкладки обоев: Rexon={} → страницы waywallen={} (Qt: {}), каталог={}; подписи плагинов={}{}",
        rexon_language,
        plan.preference,
        resolved,
        if qm_path.is_empty() {
            "исходные строки".to_string()
        } else {
            qm_path
        },
        plugin_locale,
        if l10n::normalize_ui_language(rexon_language, codes).is_none() {
            " — своего каталога у страниц на этом языке нет, они по-английски"
        } else {
            ""
        }
    );
    Some(plan.preference)
}

/// Элемент списка языков в том виде, в каком его читает вендорный QML:
/// `languages[i].code` и `languages[i].label`.
fn language_entry(code: &str, label: &QString) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(
        QString::from("code"),
        QVariant::from(&QString::from(code)),
    );
    map.insert(QString::from("label"), QVariant::from(label));
    QVariant::from(&map)
}

/// # Safety
/// `ptr` — живой менеджер соответствующего типа, созданный фабрикой.
unsafe fn pin_as<'a, T>(ptr: *mut qobject::QObject) -> Option<Pin<&'a mut T>> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { Pin::new_unchecked(&mut *ptr.cast::<T>()) })
}

impl qobject::App {
    fn renderer_manager(&self) -> *mut qobject::QObject {
        self.rust().renderer_manager
    }

    fn display_manager(&self) -> *mut qobject::QObject {
        self.rust().display_manager
    }

    fn library_manager(&self) -> *mut qobject::QObject {
        self.rust().library_manager
    }

    fn gpu_manager(&self) -> *mut qobject::QObject {
        self.rust().gpu_manager
    }

    fn presentation_manager(&self) -> *mut qobject::QObject {
        self.rust().presentation_manager
    }

    /// Дисплеи и холсты — по ним менеджер показов подписывает цели.
    fn displays_ref<'a>(&self) -> Option<&'a DisplayManager> {
        let ptr = self.rust().display_manager;
        // SAFETY: менеджер — ребёнок App и живёт столько же.
        (!ptr.is_null()).then(|| unsafe { &*ptr.cast::<DisplayManager>() })
    }

    /// Спросить у демона обоины текущих показов (`wallpaper_lookup`).
    fn request_presentation_wallpapers(mut self: Pin<&mut Self>, ids: Vec<String>) {
        // Прежний запрос, если был, относится к прежнему снимку.
        self.as_mut().rust_mut().presentation_request = 0;
        if ids.is_empty() || !we_socket::is_open() {
            return;
        }
        let token = crate::discover_queries::next_token();
        let frame = rexon_core::we_client::wallpaper_lookup(token as u64, &ids);
        let owner: *mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
        // Владельца номера запоминаем ДО отправки, как у `gpu_list`.
        unsafe { crate::we_transport::remember(token, owner.cast()) };
        self.as_mut().rust_mut().presentation_request = token;
        crate::we_transport::send(token, "wallpaper_lookup", &frame);
    }

    fn presentations_disconnected(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().presentation_request = 0;
        let manager = self.rust().presentation_manager;
        if let Some(manager) = unsafe { pin_as::<PresentationManager>(manager) } {
            manager.clear();
        }
    }

    /// Спросить у демона список видеокарт. Сокет закрыт — молчим: запрос
    /// уйдёт на подключении.
    fn request_gpu_list(mut self: Pin<&mut Self>) {
        if !we_socket::is_open() {
            return;
        }
        let token = crate::discover_queries::next_token();
        let frame = rexon_core::we_client::gpu_list(token as u64);
        let owner: *mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
        // Владельца номера запоминаем ДО отправки: ответ может прийти
        // раньше, чем мы вернёмся.
        unsafe { crate::we_transport::remember(token, owner.cast()) };
        self.as_mut().rust_mut().gpu_request = token;
        crate::we_transport::send(token, "gpu_list", &frame);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if token != 0 && self.rust().presentation_request == token {
            self.as_mut().rust_mut().presentation_request = 0;
            // Отказ и кадр обрыва обоин не несут — строки остаются с тем,
            // что уже известно.
            let Some(entries) = rexon_core::we_query::wallpaper_lookup_from_frame(frame.as_slice())
            else {
                return;
            };
            let displays = self.displays_ref();
            let manager = self.rust().presentation_manager;
            if let Some(manager) = unsafe { pin_as::<PresentationManager>(manager) } {
                manager.wallpapers_arrived(&entries, displays);
            }
            return;
        }
        if self.rust().gpu_request != token {
            return;
        }
        self.as_mut().rust_mut().gpu_request = 0;
        // Отказ демона и кадр обрыва соединения списка не несут — прежние
        // карты остаются на месте.
        let Some(gpus) = rexon_core::we_query::gpu_list_from_frame(frame.as_slice()) else {
            return;
        };
        let count = gpus.len();
        let manager = self.rust().gpu_manager;
        let changed = unsafe { pin_as::<GpuManager>(manager) }
            .is_some_and(|manager| manager.replace_all(gpus));
        rexon_core::log_info!(
            "wallpaper-tab",
            "видеокарты от демона: {count}, список {}",
            if changed { "новый" } else { "прежний" }
        );
        if changed {
            // Прежние объекты карт сняты — значки на живых страницах
            // обязаны найти свои карты заново.
            self.as_mut().gpu_manager_changed();
        }
    }

    fn ui_version(&self) -> QString {
        QString::from(&self.rust().ui_version)
    }

    /// Решение — ядро; сеанс за время работы не меняется (`CONSTANT`).
    fn apply_without_displays(&self) -> bool {
        rexon_core::waywallen_kde_plugin::apply_without_displays(we_socket::display_setup().rexon_brings_display())
    }

    /// Решение — ядро, тот же `apply_gate`, что у транспорта (`CONSTANT`).
    fn play_without_displays(&self) -> bool {
        rexon_core::waywallen_kde_plugin::play_without_displays(we_socket::display_setup().rexon_brings_display())
    }

    fn no_displays_hint(&self) -> QString {
        QString::from(&rexon_core::waywallen_kde_plugin::no_displays_hint(we_socket::display_setup()))
    }

    fn ui_language(&self) -> QString {
        QString::from(&self.rust().ui_language)
    }

    fn available_ui_languages(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        // Тот же список, что в «Настройках» Rexon: язык один на программу.
        for (code, label) in rexon_core::qt_settings::language_choices() {
            out.append(language_entry(&code, &QString::from(&label)));
        }
        out
    }

    fn set_ui_language(mut self: Pin<&mut Self>, language: &QString) -> bool {
        let (codes, module_dir, current) = {
            let rust = self.rust();
            (
                rust.ui_language_codes.clone(),
                rust.ui_module_dir.clone(),
                rust.ui_language.clone(),
            )
        };
        let wanted = rexon_core::i18n::normalize_setting(Some(&language.to_string()));
        if !rexon_core::qt_settings::language_choice_valid(&wanted) {
            return false;
        }
        // Повтор — не ошибка: кнопка «Сбросить» зовёт `setUiLanguage("system")`
        // и на уже системном языке, а `false` оборвал бы весь сброс тостом.
        if wanted == current {
            return true;
        }
        let code = rexon_core::i18n::language_for_setting(Some(&wanted));
        if apply_ui_language(&codes, &module_dir, &code).is_none() {
            return false;
        }
        let saved = rexon_core::settings::SettingsManager::new().and_then(|mut m| {
            m.set(rexon_core::i18n::SETTINGS_KEY, serde_json::Value::from(wanted.as_str()))
        });
        if let Err(e) = saved {
            rexon_core::log_warn!("lang", "язык из вкладки обоев не сохранён в настройки Rexon: {e}");
            return false;
        }
        self.as_mut().rust_mut().ui_language = wanted;
        self.as_mut().ui_language_changed();
        true
    }

    fn network_cache_size(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.rust().network_cache_size)
    }

    fn network_cache_maximum_size(&self) -> f64 {
        crate::qvalue::i64_to_qml(qobject::rexon_we_network_cache_max())
    }

    fn refresh_network_cache_size(mut self: Pin<&mut Self>) {
        let size = qobject::rexon_we_network_cache_size();
        if self.rust().network_cache_size == size {
            return;
        }
        self.as_mut().rust_mut().network_cache_size = size;
        self.as_mut().network_cache_size_changed();
    }

    fn set_network_cache_maximum_size(mut self: Pin<&mut Self>, size: f64) {
        let size = crate::qvalue::qml_to_i64(size);
        if size <= 0 || qobject::rexon_we_network_cache_max() == size {
            return;
        }
        qobject::rexon_we_set_network_cache_max(size);
        self.as_mut().network_cache_maximum_size_changed();
        self.refresh_network_cache_size();
    }

    fn clear_network_cache(self: Pin<&mut Self>) {
        qobject::rexon_we_clear_network_cache();
        self.refresh_network_cache_size();
    }

    /// Имя на шине не занимается — почему, сказано у объявления в мосте.
    fn set_single_ui_enabled(self: Pin<&mut Self>, enabled: bool) {
        let _ = (self, enabled);
    }

    fn client_error(mut self: Pin<&mut Self>, error: &QString) {
        self.as_mut().error_occurred(error.clone());
    }

    fn deliver_event(self: Pin<&mut Self>, frame: &QByteArray) {
        // Кадр разбирается ОДИН раз на все три модели: событие приходит
        // часто (fps рендерера), и три отдельных декода протобуфа были бы
        // тройной работой на ровном месте.
        let Ok(server_frame) = pb::ServerFrame::decode(frame.as_slice()) else {
            return;
        };
        let Some(pb::server_frame::Kind::Event(event)) = server_frame.kind else {
            return;
        };
        let rust = self.rust();
        let (renderers, displays, libraries) = (
            rust.renderer_manager,
            rust.display_manager,
            rust.library_manager,
        );
        if let Some(manager) = unsafe { pin_as::<RendererManager>(renderers) } {
            manager.handle_event(&event);
        }
        if let Some(manager) = unsafe { pin_as::<DisplayManager>(displays) } {
            manager.handle_event(&event);
        }
        if let Some(manager) = unsafe { pin_as::<LibraryManager>(libraries) } {
            manager.handle_event(&event);
        }
        // Показы: снимок приходит целиком (на подключении и при каждой
        // смене) и заменяет список; любое другое событие могло сменить имя
        // дисплея или холста, а по ним подписаны цели показов.
        let presentations = self.rust().presentation_manager;
        let displays_ref = self.displays_ref();
        let Some(mut manager) = (unsafe { pin_as::<PresentationManager>(presentations) }) else {
            return;
        };
        if let Some(pb::event::Payload::WallpaperPresentationSnapshot(snapshot)) = event.payload {
            let ids = manager.as_mut().replace_snapshot(snapshot.presentations, displays_ref);
            self.request_presentation_wallpapers(ids);
        } else if manager.has_presentations() {
            manager.rebuild_rows(displays_ref);
        }
    }

    fn replace_renderers(self: Pin<&mut Self>, frame: &QByteArray) {
        let renderers = self.rust().renderer_manager;
        if let Some(manager) = unsafe { pin_as::<RendererManager>(renderers) } {
            manager.replace_all_frame(frame.as_slice());
        }
    }
}
