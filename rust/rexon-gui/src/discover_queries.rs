// Запросы страницы Discover вкладки Wallpaper Engine, перенесённые с Python
// на Rust/cxx-qt: доступность источников, поиск с постраничной выдачей,
// детали предмета, скачивание и удаление.
//
// Эталон —
// rexon/qt_wallpaper_engine/qml_ui/bridge.py:873 (RemoteAvailabilityQuery),
// :936 (RemoteSearchQuery), :1206 (RemoteDetailsQuery), :1334
// (RemoteDownloadQuery). Вендорный QML (page/DiscoverPage.qml,
// page/RemoteDetailPanel.qml) НЕ трогаем, поэтому имена типов, свойств,
// сигналов и инвокаблов совпадают посимвольно.
//
// ── Три вещи, которые определяют устройство этого файла ────────────────────
//
// 1. Наследования нет. cxx-qt не умеет наследование Rust-QObject от
//    Rust-QObject, а у оригинала все запросы наследуют одну базу `Query`
//    (qextra::QAsyncResult). База перенесена СОСТОЯНИЕМ: `QueryState` из
//    rexon-core::we_query встраивается полем в каждый запрос, а одинаковые
//    свойства/сигналы порождает макрос `impl_query_base!` — Qt-наследование
//    заменяется композицией плюс кодогенерацией, как и записано в плане.
//
// 2. Транспорт ещё Python. `client.py` (QWebSocket) переезжает только в
//    партии 7, поэтому запрос НЕ отправляет кадр сам: он собирает готовые
//    байты `Request` и отдаёт их сигналом `sendRequest(token, field, bytes)`,
//    а ответ получает инвокаблом `deliverResponse(token, frame)`. Токен —
//    наш собственный номер, по нему запрос помнит, какую страницу он просил
//    и какой предмет качал (в Python это делали замыкания `_done`).
//    Живучесть: обращения к мёртвому объекту тут нет вовсе — доставку
//    выполняет вызывающий, а не колбэк по сохранённому указателю.
//
// 3. Дебаунс — настоящий. Смена `sourceId`/`query`/`sortKey`/`tags` не шлёт
//    запрос сразу: как у оригинала (qextra query.cpp:16), взводится
//    одноразовый таймер на 100 мс, и подряд идущие правки склеиваются в ОДИН
//    запрос первой страницы. Таймер — C++-шим (обёртки QTimer в cxx-qt-lib
//    нет), см. cpp/rexon_gui_factory.cpp.
//
// ── Что в эту партию СОЗНАТЕЛЬНО не входит ─────────────────────────────────
//
// Ветки Мастерской Steam (`_open_workshop`, `_fetch_page_workshop`,
// `_reload_workshop`, `_fetch_authors`, `_watch_workshop_download`) остаются
// на Python: они ходят в Steam синхронной сетью из фонового потока, а по
// плану Мастерская — партия 6. Здесь
// реализован путь демона; развилку `workshop_serves()` шим-владелец делает
// снаружи, как и сейчас.
//
// ⚠️ Сейчас эта развилка ВЫКЛЮЧЕНА (`WORKSHOP_SERVED_LOCALLY = False` в
// шиме): каталог Мастерской снова отдаёт плагин демона, как у вендора, —
// решение Wehrwolfmann «идти вплотную за апстримом». Портировать те ветки
// незачем, пока решение не пересмотрят.
//
// Разделение слоёв то же, что в партии 2: всё состояние и все решения
// «какой сигнал и в каком порядке» живут в чистых структурах без Qt (их и
// проверяет голден против ЖИВЫХ Python-запросов), а QObject только исполняет
// полученный список команд.

/// Мост cxx-qt: четыре объекта-запроса страницы Discover.
///
/// Все четыре в ОДНОМ мосте намеренно: правило «один мост на файл» — про
/// файл, а не про число QObject'ов, и общая база у них одна. Побочный эффект
/// нескольких `#[qobject]` в мосте — шорткат `Self` в сигнатурах перестаёт
/// работать, поэтому всюду пишется полное имя типа.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        /// QVariantList — им уезжает `sources` (список описаний источников).
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    // Фабрика и таймер дебаунса: `new` живёт только в C++.
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// # Safety
        /// parent либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_search_result_model(parent: *mut QObject) -> *mut QObject;

        /// Значение QVariant в JSON правилами самого Qt — им стенд сверяет
        /// то, что увидит QML.
        fn rexon_variant_to_json(value: &QVariant) -> QByteArray;
        /// # Safety
        /// owner обязан быть живым QObject с Q_INVOKABLE `reload()`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_reload_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;
        /// # Safety
        /// timer обязан быть живым QTimer, созданным фабрикой выше.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_debounce(timer: *mut QObject);

        // Фабрики самих запросов. Из QML их создаёт движок, здесь они нужны
        // стенду (и будущему шиму императивной регистрации): `new` для
        // cxx-qt-объекта существует только в C++.
        /// # Safety
        /// parent либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_availability_query(parent: *mut QObject) -> *mut QObject;
        /// # Safety
        /// parent либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_search_query(parent: *mut QObject) -> *mut QObject;
        /// # Safety
        /// parent либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_details_query(parent: *mut QObject) -> *mut QObject;
        /// # Safety
        /// parent либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_download_query(parent: *mut QObject) -> *mut QObject;
    }

    // ── RemoteAvailabilityQuery ───────────────────────────────────────────
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        #[qproperty(QList_QVariant, sources, READ = sources, NOTIFY = sources_changed)]
        #[qproperty(QString, default_source_id, cxx_name = "defaultSourceId",
                    READ = default_source_id, NOTIFY = sources_changed)]
        type RemoteAvailabilityQuery = super::RemoteAvailabilityQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        // Модель отдаётся как QObject*: ListView принимает QVariant, а Qt сам
        // распознаёт в нём модель. CONSTANT — как `constant=True` у моста.
        #[qproperty(*mut QObject, model, READ = model, CONSTANT)]
        #[qproperty(QString, source_id, cxx_name = "sourceId",
                    READ = source_id, WRITE = set_source_id,
                    NOTIFY = source_id_changed)]
        #[qproperty(QString, query, READ = query, WRITE = set_query,
                    NOTIFY = query_changed)]
        #[qproperty(QString, sort_key, cxx_name = "sortKey",
                    READ = sort_key, WRITE = set_sort_key,
                    NOTIFY = sort_key_changed)]
        #[qproperty(QStringList, tags, READ = tags, WRITE = set_tags,
                    NOTIFY = tags_changed)]
        #[qproperty(bool, browsing_enabled, cxx_name = "browsingEnabled",
                    READ = browsing_enabled, WRITE = set_browsing_enabled,
                    NOTIFY = browsing_enabled_changed)]
        #[qproperty(bool, has_more, cxx_name = "hasMore",
                    READ = has_more, NOTIFY = state_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText",
                    READ = error_text, NOTIFY = state_changed)]
        type RemoteSearchQuery = super::RemoteSearchQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        #[qproperty(QString, source_id, cxx_name = "sourceId",
                    READ = source_id, WRITE = set_source_id,
                    NOTIFY = source_id_changed)]
        #[qproperty(QString, item_id, cxx_name = "itemId",
                    READ = item_id, WRITE = set_item_id,
                    NOTIFY = item_id_changed)]
        #[qproperty(QString, author, READ = author, NOTIFY = loaded)]
        /// waywallen 0.5.0: аватар автора (`remote_query.cppm`).
        #[qproperty(QString, author_avatar_url, cxx_name = "authorAvatarUrl",
                    READ = author_avatar_url, NOTIFY = loaded)]
        #[qproperty(QString, description, READ = description, NOTIFY = loaded)]
        #[qproperty(QString, size, READ = size, NOTIFY = loaded)]
        #[qproperty(i32, width, READ = width, NOTIFY = loaded)]
        #[qproperty(i32, height, READ = height, NOTIFY = loaded)]
        #[qproperty(QStringList, tags, READ = tags, NOTIFY = loaded)]
        #[qproperty(QString, web_url, cxx_name = "webUrl",
                    READ = web_url, NOTIFY = loaded)]
        type RemoteDetailsQuery = super::RemoteDetailsQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        type RemoteDownloadQuery = super::RemoteDownloadQueryRust;
    }

    // ── Сигналы базы Query (одинаковые у всех четырёх) ────────────────────
    //
    // statusChanged несёт статус: обработчики вендорного QML написаны как
    // `function onStatusChanged(status)`. appErrorOccurred — глобальная
    // пересылка ошибки; у оригинала её шлёт синглтон App, которого на Rust
    // ещё нет (партия 5), поэтому запрос отдаёт её своим сигналом, а шим
    // соединяет с App. Отдельный сигнал, а не молчание: без него ошибка
    // источника не доехала бы до тоста в Window.qml.
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteAvailabilityQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteAvailabilityQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteAvailabilityQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteAvailabilityQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteAvailabilityQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteAvailabilityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteAvailabilityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sourcesChanged"]
        fn sources_changed(self: Pin<&mut RemoteAvailabilityQuery>);
        /// Кадр к демону: токен, поле запроса, готовые байты `Request`.
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut RemoteAvailabilityQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteSearchQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteSearchQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteSearchQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sourceIdChanged"]
        fn source_id_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "queryChanged"]
        fn query_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "sortKeyChanged"]
        fn sort_key_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "tagsChanged"]
        fn tags_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "browsingEnabledChanged"]
        fn browsing_enabled_changed(self: Pin<&mut RemoteSearchQuery>);
        /// Состояние выдачи: `hasMore`, `errorText` и всё, что зависит от
        /// страницы. Один сигнал на всё — так у моста и у оригинала.
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut RemoteSearchQuery>);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut RemoteSearchQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteDetailsQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteDetailsQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteDetailsQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sourceIdChanged"]
        fn source_id_changed(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        #[cxx_name = "itemIdChanged"]
        fn item_id_changed(self: Pin<&mut RemoteDetailsQuery>);
        /// Поля деталей перечитаны (в том числе сброшены перед запросом).
        #[qsignal]
        fn loaded(self: Pin<&mut RemoteDetailsQuery>);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut RemoteDetailsQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut RemoteDownloadQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut RemoteDownloadQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut RemoteDownloadQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut RemoteDownloadQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut RemoteDownloadQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut RemoteDownloadQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut RemoteDownloadQuery>, error: QString);
        /// Демон принял задание на скачивание.
        #[qsignal]
        fn accepted(self: Pin<&mut RemoteDownloadQuery>, source_id: QString, id: QString);
        /// Демон отказал (или запрос не дошёл).
        #[qsignal]
        fn rejected(
            self: Pin<&mut RemoteDownloadQuery>,
            source_id: QString,
            id: QString,
            error: QString,
        );
        /// Предмет удалён с диска.
        #[qsignal]
        fn removed(self: Pin<&mut RemoteDownloadQuery>, source_id: QString, id: QString);
        #[qsignal]
        #[cxx_name = "removeFailed"]
        fn remove_failed(
            self: Pin<&mut RemoteDownloadQuery>,
            source_id: QString,
            id: QString,
            error: QString,
        );
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut RemoteDownloadQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );
    }

    // ── Свойства и API ────────────────────────────────────────────────────
    extern "RustQt" {
        fn status(self: &RemoteAvailabilityQuery) -> i32;
        fn error(self: &RemoteAvailabilityQuery) -> QString;
        fn querying(self: &RemoteAvailabilityQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteAvailabilityQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteAvailabilityQuery>, value: bool);
        fn sources(self: &RemoteAvailabilityQuery) -> QList_QVariant;
        #[cxx_name = "defaultSourceId"]
        fn default_source_id(self: &RemoteAvailabilityQuery) -> QString;

        fn status(self: &RemoteSearchQuery) -> i32;
        fn error(self: &RemoteSearchQuery) -> QString;
        fn querying(self: &RemoteSearchQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteSearchQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteSearchQuery>, value: bool);
        fn model(self: &RemoteSearchQuery) -> *mut QObject;
        #[cxx_name = "sourceId"]
        fn source_id(self: &RemoteSearchQuery) -> QString;
        #[cxx_name = "setSourceId"]
        fn set_source_id(self: Pin<&mut RemoteSearchQuery>, value: QString);
        fn query(self: &RemoteSearchQuery) -> QString;
        #[cxx_name = "setQuery"]
        fn set_query(self: Pin<&mut RemoteSearchQuery>, value: QString);
        #[cxx_name = "sortKey"]
        fn sort_key(self: &RemoteSearchQuery) -> QString;
        #[cxx_name = "setSortKey"]
        fn set_sort_key(self: Pin<&mut RemoteSearchQuery>, value: QString);
        fn tags(self: &RemoteSearchQuery) -> QStringList;
        #[cxx_name = "setTags"]
        fn set_tags(self: Pin<&mut RemoteSearchQuery>, value: QStringList);
        #[cxx_name = "browsingEnabled"]
        fn browsing_enabled(self: &RemoteSearchQuery) -> bool;
        #[cxx_name = "setBrowsingEnabled"]
        fn set_browsing_enabled(self: Pin<&mut RemoteSearchQuery>, value: bool);
        #[cxx_name = "hasMore"]
        fn has_more(self: &RemoteSearchQuery) -> bool;
        #[cxx_name = "errorText"]
        fn error_text(self: &RemoteSearchQuery) -> QString;

        fn status(self: &RemoteDetailsQuery) -> i32;
        fn error(self: &RemoteDetailsQuery) -> QString;
        fn querying(self: &RemoteDetailsQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteDetailsQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteDetailsQuery>, value: bool);
        #[cxx_name = "sourceId"]
        fn source_id(self: &RemoteDetailsQuery) -> QString;
        #[cxx_name = "setSourceId"]
        fn set_source_id(self: Pin<&mut RemoteDetailsQuery>, value: QString);
        #[cxx_name = "itemId"]
        fn item_id(self: &RemoteDetailsQuery) -> QString;
        #[cxx_name = "setItemId"]
        fn set_item_id(self: Pin<&mut RemoteDetailsQuery>, value: QString);
        fn author(self: &RemoteDetailsQuery) -> QString;
        #[cxx_name = "authorAvatarUrl"]
        fn author_avatar_url(self: &RemoteDetailsQuery) -> QString;
        fn description(self: &RemoteDetailsQuery) -> QString;
        fn size(self: &RemoteDetailsQuery) -> QString;
        fn width(self: &RemoteDetailsQuery) -> i32;
        fn height(self: &RemoteDetailsQuery) -> i32;
        fn tags(self: &RemoteDetailsQuery) -> QStringList;
        #[cxx_name = "webUrl"]
        fn web_url(self: &RemoteDetailsQuery) -> QString;

        fn status(self: &RemoteDownloadQuery) -> i32;
        fn error(self: &RemoteDownloadQuery) -> QString;
        fn querying(self: &RemoteDownloadQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &RemoteDownloadQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut RemoteDownloadQuery>, value: bool);
    }

    // ── Инвокаблы ─────────────────────────────────────────────────────────
    extern "RustQt" {
        /// Перезагрузка запроса. Имя обязано остаться `reload`: по нему в
        /// него стучится и вендорный QML, и таймер дебаунса
        /// (QMetaObject::invokeMethod по строке).
        #[qinvokable]
        fn reload(self: Pin<&mut RemoteAvailabilityQuery>);
        /// Ответ демона: сырые байты кадра `ServerFrame`.
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(
            self: Pin<&mut RemoteAvailabilityQuery>,
            token: i64,
            frame: &QByteArray,
        );

        #[qinvokable]
        fn reload(self: Pin<&mut RemoteSearchQuery>);
        /// Догрузить следующую страницу вручную (кнопка QML).
        #[qinvokable]
        #[cxx_name = "loadMore"]
        fn load_more(self: Pin<&mut RemoteSearchQuery>);
        /// Догрузка по протоколу модели. Аргумент — строка, с которой
        /// продолжать; оригинал его тоже игнорирует (страница считается
        /// счётчиком `offset`).
        #[qinvokable]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut RemoteSearchQuery>, row: i32);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteSearchQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut RemoteDetailsQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteDetailsQuery>, token: i64, frame: &QByteArray);

        /// Скачать (для источника с подписками — подписаться).
        #[qinvokable]
        fn start(self: Pin<&mut RemoteDownloadQuery>, source_id: &QString, id: &QString);
        /// Удалить скачанное. Имя вендорное (DiscoverPage.qml:357).
        #[qinvokable]
        fn remove(self: Pin<&mut RemoteDownloadQuery>, source_id: &QString, id: &QString);
        /// Прежнее имя моста — вендор зовёт remove().
        #[qinvokable]
        fn uninstall(self: Pin<&mut RemoteDownloadQuery>, source_id: &QString, id: &QString);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut RemoteDownloadQuery>, token: i64, frame: &QByteArray);

        /// Состояние соединения с демоном — общее на все запросы, как
        /// модульная переменная `_client` в мосте. Инвокабл на каждом
        /// запросе, чтобы шиму было куда его позвать с любой страницы.
        #[qinvokable]
        #[cxx_name = "setTransportConnected"]
        fn set_transport_connected(self: &RemoteAvailabilityQuery, value: bool);
        #[qinvokable]
        #[cxx_name = "setTransportConnected"]
        fn set_transport_connected(self: &RemoteSearchQuery, value: bool);
        #[qinvokable]
        #[cxx_name = "setTransportConnected"]
        fn set_transport_connected(self: &RemoteDetailsQuery, value: bool);
        #[qinvokable]
        #[cxx_name = "setTransportConnected"]
        fn set_transport_connected(self: &RemoteDownloadQuery, value: bool);
    }

    impl cxx_qt::Initialize for RemoteAvailabilityQuery {}
    impl cxx_qt::Initialize for RemoteSearchQuery {}
    impl cxx_qt::Initialize for RemoteDetailsQuery {}
    impl cxx_qt::Initialize for RemoteDownloadQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QList, QString, QStringList, QVariant};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use prost::Message;
use serde_json::{Map, Value};

use rexon_core::proto::control as pb;
use rexon_core::remote_l10n::Localizer;
use rexon_core::we_client;
use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR, STATUS_FINISHED};

use crate::remote_list_model::{acquisition, RemoteRow};
use crate::qvalue::json_to_variant;
use crate::we_transport;

// ============================================================================
// Общее окружение запросов (в Python — модульные переменные моста)
// ============================================================================

/// Идентификатор источника «Мастерская Steam» (bridge.py:376). Нужен
/// значению по умолчанию страницы; сами запросы Мастерской — партия 6.
pub const WORKSHOP_SOURCE_ID: &str = "wallpaper_engine";

/// Есть ли живое соединение с демоном. В Python это `_client.is_open()`, то
/// есть тоже одна переменная на весь мост; здесь — атомик, потому что
/// выставлять её будет транспорт.
static CONNECTED: AtomicBool = AtomicBool::new(true);

/// Живо ли соединение с демоном.
pub fn connected() -> bool {
    CONNECTED.load(Ordering::Relaxed)
}

/// Сообщить запросам о состоянии соединения (зовёт транспорт).
pub fn set_connected(value: bool) {
    CONNECTED.store(value, Ordering::Relaxed);
}

/// Номер запроса. Он же `request_id` протокола — счётчик ОДИН на процесс и
/// живёт в транспорте (`we_socket`), поверх `we_client::RequestIds` ядра.
///
/// ⚠️ Раньше номеров было два: свой токен здесь и свой `request_id` у
/// питоновского клиента, который ради этого разбирал готовый кадр `prost`-ом
/// и собирал обратно. Двойная нумерация и двойной разбор ушли вместе с
/// переездом сокета на Rust.
pub fn next_token() -> i64 {
    crate::we_socket::alloc_request_id()
}

/// Начать нумерацию заново (нужно стенду: голден нумерует кадры с единицы).
pub fn reset_tokens() {
    crate::we_socket::reset_request_ids();
}

/// Замок нумерации для сверок крейта. Счётчик один на процесс, а голдены
/// Discover и страницы «Обои» оба сбрасывают его и ждут кадры с единицы:
/// идя параллельно, они перехватывали номера друг у друга, и одна из
/// сверок изредка падала на «некому отдать ответ 2».
#[cfg(test)]
pub(crate) fn token_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // Упавшая сверка не должна валить соседку «отравленным» замком.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Переводчик описаний источников. Один на мост — как модуль
/// `qml_ui/remote_l10n` в Python: карты обратного перевода тегов наполняет
/// запрос доступности, а пользуется ими запрос поиска.
fn localizer() -> &'static Mutex<Localizer> {
    static L10N: OnceLock<Mutex<Localizer>> = OnceLock::new();
    L10N.get_or_init(|| Mutex::new(Localizer::new()))
}

/// Забыть накопленные карты обратного перевода (стенду — между случаями).
pub fn reset_localizer() {
    *localizer().lock().expect("переводчик не отравлен") = Localizer::new();
}

// ============================================================================
// Чистый слой: наблюдаемые события и команды окружению
// ============================================================================

/// Наблюдаемый сигнал запроса. Порядок этих событий — такая же часть
/// контракта, как значения свойств: голден снят с ЖИВЫХ Python-запросов
/// именно последовательностью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `statusChanged(Status)`.
    Status(i32),
    /// `queryingChanged()`.
    Querying,
    /// `errorChanged()`.
    ErrorChanged,
    /// `forwardErrorChanged()`.
    ForwardErrorChanged,
    /// `finished()`.
    Finished,
    /// `errorOccurred(error)`.
    ErrorOccurred(String),
    /// Глобальная пересылка ошибки (у оригинала — `App.errorOccurred`).
    AppError(String),
    /// Сигнал самого запроса: `sourcesChanged`, `stateChanged`, `loaded`…
    Prop(&'static str),
    /// Кадр, уходящий демону.
    Sent {
        token: i64,
        field: &'static str,
        request: Request,
    },
    /// `accepted(sourceId, id)`.
    Accepted { source_id: String, id: String },
    /// `rejected(sourceId, id, error)`.
    Rejected {
        source_id: String,
        id: String,
        error: String,
    },
    /// `removed(sourceId, id)`.
    Removed { source_id: String, id: String },
    /// `removeFailed(sourceId, id, error)`.
    RemoveFailed {
        source_id: String,
        id: String,
        error: String,
    },
}

/// Действие запроса над окружением: моделью, общим складом, таймером.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Наблюдаемый сигнал.
    Signal(Event),
    /// Команда дочерней модели выдачи.
    Model(ModelCmd),
    /// Разослать состояние приобретения через общий склад.
    Store {
        source_id: String,
        id: String,
        state: i32,
    },
    /// Взвести таймер отложенной перезагрузки (100 мс).
    RequestReload,
}

/// Что запрос просит сделать со своей моделью выдачи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCmd {
    /// `setHasMore` до отправки страницы — чтобы вид не дёргал догрузку.
    SetHasMore(bool),
    /// Полная замена выдачи.
    Reset { rows: Vec<RemoteRow>, has_more: bool },
    /// Догрузка страницы.
    Append { rows: Vec<RemoteRow>, has_more: bool },
}

/// Кадр запроса к демону в разобранном виде — так его и сверяет голден.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Availability,
    Search {
        source_id: String,
        query: String,
        sort_key: String,
        page: u32,
        required_tags: Vec<String>,
    },
    Details {
        source_id: String,
        id: String,
    },
    Download {
        source_id: String,
        id: String,
    },
    Uninstall {
        source_id: String,
        id: String,
    },
}

impl Request {
    /// Поле `oneof payload`, в которое ложится этот запрос.
    pub fn field(&self) -> &'static str {
        match self {
            Request::Availability => "remote_availability",
            Request::Search { .. } => "remote_search",
            Request::Details { .. } => "remote_details",
            Request::Download { .. } => "remote_download",
            Request::Uninstall { .. } => "remote_uninstall",
        }
    }

    /// Готовые байты `Request` протокола. `request_id` оставлен нулевым:
    /// его выдаёт транспорт, у которого свой счётчик (`RequestIds`).
    /// Байты кадра `Request` с уже проставленным номером.
    ///
    /// Номер приходит параметром, а не нулём: раньше его подставлял клиент
    /// на Python, разбирая и пересобирая готовый кадр. Теперь номер один и
    /// проставляется там же, где кадр собран.
    pub fn encode(&self, request_id: u64) -> Vec<u8> {
        let rid = request_id;
        match self {
            Request::Availability => we_client::remote_availability(rid),
            Request::Search {
                source_id,
                query,
                sort_key,
                page,
                required_tags,
            } => we_client::remote_search(rid, query, sort_key, *page, source_id, required_tags),
            Request::Details { source_id, id } => we_client::remote_details(rid, id, source_id),
            Request::Download { source_id, id } => we_client::remote_download(rid, id, source_id),
            Request::Uninstall { source_id, id } => we_client::remote_uninstall(rid, id, source_id),
        }
    }

    /// Поля кадра в том же виде, в каком их печатает голден (имена — из
    /// protobuf, включая значения по умолчанию).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        match self {
            Request::Availability => {}
            Request::Search {
                source_id,
                query,
                sort_key,
                page,
                required_tags,
            } => {
                map.insert("source_id".into(), Value::String(source_id.clone()));
                map.insert("query".into(), Value::String(query.clone()));
                map.insert("sort_key".into(), Value::String(sort_key.clone()));
                map.insert("page".into(), Value::from(*page));
                map.insert(
                    "required_tags".into(),
                    Value::Array(
                        required_tags
                            .iter()
                            .map(|t| Value::String(t.clone()))
                            .collect(),
                    ),
                );
            }
            Request::Details { source_id, id }
            | Request::Download { source_id, id }
            | Request::Uninstall { source_id, id } => {
                map.insert("source_id".into(), Value::String(source_id.clone()));
                map.insert("id".into(), Value::String(id.clone()));
            }
        }
        Value::Object(map)
    }

    /// Разбор кадра, собранного `encode()` — им стенд сверяет байты, которые
    /// реально уехали сигналом, а не только намерение их отправить.
    pub fn decode(bytes: &[u8]) -> Option<Request> {
        let request = pb::Request::decode(bytes).ok()?;
        Some(match request.payload? {
            pb::request::Payload::RemoteAvailability(_) => Request::Availability,
            pb::request::Payload::RemoteSearch(s) => Request::Search {
                source_id: s.source_id,
                query: s.query,
                sort_key: s.sort_key,
                page: s.page,
                required_tags: s.required_tags,
            },
            pb::request::Payload::RemoteDetails(d) => Request::Details {
                source_id: d.source_id,
                id: d.id,
            },
            pb::request::Payload::RemoteDownload(d) => Request::Download {
                source_id: d.source_id,
                id: d.id,
            },
            pb::request::Payload::RemoteUninstall(u) => Request::Uninstall {
                source_id: u.source_id,
                id: u.id,
            },
            _ => return None,
        })
    }
}

/// Сигналы базы Query → наблюдаемые события.
fn base_events(signals: Vec<QuerySignal>) -> Vec<Cmd> {
    signals
        .into_iter()
        .map(|s| {
            Cmd::Signal(match s {
                QuerySignal::StatusChanged(v) => Event::Status(v),
                QuerySignal::QueryingChanged => Event::Querying,
                QuerySignal::ErrorChanged => Event::ErrorChanged,
                QuerySignal::ForwardErrorChanged => Event::ForwardErrorChanged,
                QuerySignal::Finished => Event::Finished,
                QuerySignal::ErrorOccurred(e) => Event::ErrorOccurred(e),
                QuerySignal::AppErrorOccurred(e) => Event::AppError(e),
                // Свойства простых запросов ядра сюда не приходят: разбор
                // ответов Discover живёт здесь и шлёт Event::Prop сам.
                QuerySignal::Property(name) => Event::Prop(name),
            })
        })
        .collect()
}

/// Снять `Response` с сырого кадра и обработать не-OK статус.
///
/// Копия `we_query::take_response`, которая в ядре объявлена `pub(crate)`.
/// Дублируется сознательно: разбор ответов Discover по плану живёт в
/// GUI-крейте (партия 3), а расширять поверхность ядра ради одной функции —
/// значит трогать чужую зону.
fn take_response(
    state: &mut QueryState,
    frame_bytes: &[u8],
) -> Result<(Option<pb::Response>, Vec<Cmd>), prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    let Some(pb::server_frame::Kind::Response(rsp)) = frame.kind else {
        return Err(prost::DecodeError::new("кадр сервера без ответа"));
    };
    if rsp.status != pb::Status::Ok as i32 {
        let text = we_query::response_error_text(rsp.status, &rsp.message);
        let mut out = we_query::set_error(state, &text);
        out.extend(we_query::set_status(state, STATUS_ERROR));
        return Ok((None, base_events(out)));
    }
    Ok((Some(rsp), Vec::new()))
}

/// Общий хвост успешного разбора: свойства обновлены — статус в Finished.
fn finish(state: &mut QueryState) -> Vec<Cmd> {
    base_events(we_query::set_status(state, STATUS_FINISHED))
}

/// `setForwardError` базы — одинаков у всех четырёх запросов.
fn set_forward_error_pure(state: &mut QueryState, value: bool) -> Vec<Cmd> {
    base_events(we_query::set_forward_error(state, value))
}

// ============================================================================
// RemoteAvailabilityQuery — список источников каталога
// ============================================================================

/// Карта `PluginMessage` для QML: `{pluginId, msgid}`.
///
/// С waywallen 0.3.7 подписи плагинов ездят парой «плагин + исходная
/// строка», а вендорный QML показывает их через `I18n.tr()` →
/// `PluginTranslations`. Пустой `msgid` — это «подписи нет»: карта всё
/// равно кладётся, чтобы у QML был предсказуемый тип поля, а запасной
/// текст он выберет сам (`I18n.tr(x) || modelData.name`).
fn plugin_message(message: Option<&pb::PluginMessage>) -> Value {
    let mut map = Map::new();
    let (plugin_id, msgid, arguments) = match message {
        Some(m) => (m.plugin_id.clone(), m.msgid.clone(), m.arguments.clone()),
        None => (String::new(), String::new(), Vec::new()),
    };
    map.insert("pluginId".into(), Value::String(plugin_id));
    map.insert("msgid".into(), Value::String(msgid));
    // waywallen 0.3.8: подпись плагина может нести подстановки (`%1`, `%2`).
    // Ключ кладётся всегда — у QML предсказуемый тип поля.
    map.insert(
        "arguments".into(),
        Value::Array(arguments.into_iter().map(Value::String).collect()),
    );
    Value::Object(map)
}

/// `PluginMessage` с запасным ТЕКСТОМ, как `pluginMessageFromPb` вендора.
///
/// Отличие от [`plugin_message`] намеренное и нужно ровно там, где вендорный
/// QML выбирает поле оператором `??`, а не `||`: у варианта фильтра он пишет
/// `W.I18n.tr(option?.labelText ?? option?.label ?? value)`. Пустая карта —
/// это ЗНАЧЕНИЕ, `??` её не пропускает, и подпись выродилась бы в
/// техническую строку запроса. Поэтому «подписи нет» здесь выражается
/// строкой-запаской, как у вендора, а не пустой картой.
fn plugin_message_or(message: Option<&pb::PluginMessage>, fallback: &str) -> Value {
    let empty = message.is_none_or(|m| m.plugin_id.is_empty() || m.msgid.is_empty());
    if empty {
        return Value::String(fallback.to_string());
    }
    plugin_message(message)
}

/// `PluginMessage` источника, у которого запасной текст вендора не совпадает
/// с тем, что выберет сам QML.
///
/// Вендор отдаёт `pluginMessageFromPb(сообщение, запасной текст)`. Обычно
/// пустой карты хватает: разметка дописывает запаску сама
/// (`I18n.tr(x.labelText) || x.label`). У двух полей источника это не так:
/// имя страница берёт как `I18n.tr(s.displayNameText) || s.name` — то есть
/// техническое `wallhaven` вместо `Wallhaven` из `displayName`, — а подсказку
/// `remoteHintText` показывает вовсе без запаски. Здесь запасной текст
/// кладётся в `msgid` при пустом плагине: каталог по нему не ищется, наш
/// ключевой перевод (`rexonFallback`) по-прежнему идёт первым, иначе строка
/// выходит как есть. Поле остаётся картой — тип у QML прежний.
fn plugin_message_text(message: Option<&pb::PluginMessage>, fallback: &str) -> Value {
    let empty = message.is_none_or(|m| m.plugin_id.is_empty() || m.msgid.is_empty());
    if !empty || fallback.is_empty() {
        return plugin_message(message);
    }
    let mut map = Map::new();
    map.insert("pluginId".into(), Value::String(String::new()));
    map.insert("msgid".into(), Value::String(fallback.to_string()));
    map.insert("arguments".into(), Value::Array(Vec::new()));
    Value::Object(map)
}

/// Описание источника в виде карты для QML (контракт `remote_query.cpp`).
///
/// ⚠️ Порт (правило #16) сохраняет починку моста: отдаются ВСЕ пятнадцать
/// полей, а не шесть. Без `filters`/`actions`/`remoteCapability`/`displayName`
/// у вендорной страницы Discover молча не работали диалог фильтров, кнопка
/// входа, кнопка подписки и человеческое имя источника.
fn source_info(s: &pb::RemoteSourceInfo) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(s.id.clone()));
    map.insert("name".into(), Value::String(s.name.clone()));
    map.insert(
        "displayName".into(),
        Value::String(if s.display_name.is_empty() {
            s.name.clone()
        } else {
            s.display_name.clone()
        }),
    );
    map.insert(
        "displayNameText".into(),
        plugin_message_text(s.display_name_text.as_ref(), &s.display_name),
    );
    map.insert("supportsSearch".into(), Value::Bool(s.supports_search));
    map.insert(
        "sorts".into(),
        Value::Array(
            s.sorts
                .iter()
                .map(|o| {
                    let mut item = Map::new();
                    item.insert("key".into(), Value::String(o.key.clone()));
                    item.insert("label".into(), Value::String(o.label.clone()));
                    item.insert("labelText".into(), plugin_message(o.label_text.as_ref()));
                    Value::Object(item)
                })
                .collect(),
        ),
    );
    map.insert(
        "tags".into(),
        Value::Array(s.tags.iter().map(|t| Value::String(t.clone())).collect()),
    );
    map.insert("contentDir".into(), Value::String(s.content_dir.clone()));
    map.insert(
        "ownerPluginId".into(),
        Value::String(s.owner_plugin_id.clone()),
    );
    map.insert("remoteCapability".into(), Value::from(s.remote_capability));
    map.insert("remoteHint".into(), Value::String(s.remote_hint.clone()));
    map.insert(
        "remoteHintText".into(),
        plugin_message_text(s.remote_hint_text.as_ref(), &s.remote_hint),
    );
    map.insert("avatarUrl".into(), Value::String(s.avatar_url.clone()));
    map.insert(
        "filters".into(),
        Value::Array(
            s.filters
                .iter()
                .map(|f| {
                    let mut item = Map::new();
                    item.insert("id".into(), Value::String(f.id.clone()));
                    item.insert("title".into(), Value::String(f.title.clone()));
                    item.insert("type".into(), Value::from(f.r#type));
                    // waywallen 0.3.8: вместо голого списка строк у варианта
                    // фильтра теперь тройка «значение / подпись / подпись
                    // плагина». В запрос уходит `value`, человеку показывается
                    // подпись (правило #13).
                    item.insert(
                        "options".into(),
                        Value::Array(
                            f.options
                                .iter()
                                .map(|o| {
                                    let label = if o.label.is_empty() {
                                        o.value.clone()
                                    } else {
                                        o.label.clone()
                                    };
                                    let mut opt = Map::new();
                                    opt.insert("value".into(), Value::String(o.value.clone()));
                                    opt.insert("label".into(), Value::String(label.clone()));
                                    opt.insert(
                                        "labelText".into(),
                                        plugin_message_or(o.label_text.as_ref(), &label),
                                    );
                                    Value::Object(opt)
                                })
                                .collect(),
                        ),
                    );
                    item.insert("description".into(), Value::String(f.description.clone()));
                    item.insert(
                        "confirmation".into(),
                        Value::String(f.confirmation.clone()),
                    );
                    item.insert("titleText".into(), plugin_message(f.title_text.as_ref()));
                    item.insert(
                        "descriptionText".into(),
                        plugin_message(f.description_text.as_ref()),
                    );
                    item.insert(
                        "confirmationText".into(),
                        plugin_message(f.confirmation_text.as_ref()),
                    );
                    Value::Object(item)
                })
                .collect(),
        ),
    );
    map.insert(
        "actions".into(),
        Value::Array(
            s.actions
                .iter()
                .map(|a| {
                    let mut item = Map::new();
                    item.insert("id".into(), Value::String(a.id.clone()));
                    item.insert("label".into(), Value::String(a.label.clone()));
                    item.insert("group".into(), Value::String(a.group.clone()));
                    item.insert("order".into(), Value::from(a.order));
                    item.insert("kind".into(), Value::from(a.kind));
                    item.insert("visible".into(), Value::Bool(a.visible));
                    item.insert("enabled".into(), Value::Bool(a.enabled));
                    item.insert("description".into(), Value::String(a.description.clone()));
                    item.insert(
                        "requiredForBrowsing".into(),
                        Value::Bool(a.required_for_browsing),
                    );
                    item.insert(
                        "browseDescription".into(),
                        Value::String(a.browse_description.clone()),
                    );
                    item.insert(
                        "browseButtonLabel".into(),
                        Value::String(a.browse_button_label.clone()),
                    );
                    item.insert("labelText".into(), plugin_message(a.label_text.as_ref()));
                    item.insert(
                        "descriptionText".into(),
                        plugin_message(a.description_text.as_ref()),
                    );
                    item.insert(
                        "browseDescriptionText".into(),
                        plugin_message(a.browse_description_text.as_ref()),
                    );
                    item.insert(
                        "browseButtonLabelText".into(),
                        plugin_message(a.browse_button_label_text.as_ref()),
                    );
                    item.insert("groupLabel".into(), plugin_message(a.group_label.as_ref()));
                    item.insert(
                        "fields".into(),
                        Value::Array(
                            a.fields
                                .iter()
                                .map(|f| {
                                    let mut fl = Map::new();
                                    fl.insert("key".into(), Value::String(f.key.clone()));
                                    fl.insert("label".into(), Value::String(f.label.clone()));
                                    fl.insert(
                                        "description".into(),
                                        Value::String(f.description.clone()),
                                    );
                                    fl.insert(
                                        "placeholder".into(),
                                        Value::String(f.placeholder.clone()),
                                    );
                                    fl.insert("secret".into(), Value::Bool(f.secret));
                                    fl.insert("required".into(), Value::Bool(f.required));
                                    fl.insert(
                                        "labelText".into(),
                                        plugin_message(f.label_text.as_ref()),
                                    );
                                    fl.insert(
                                        "descriptionText".into(),
                                        plugin_message(f.description_text.as_ref()),
                                    );
                                    fl.insert(
                                        "placeholderText".into(),
                                        plugin_message(f.placeholder_text.as_ref()),
                                    );
                                    Value::Object(fl)
                                })
                                .collect(),
                        ),
                    );
                    Value::Object(item)
                })
                .collect(),
        ),
    );
    map.insert(
        "status".into(),
        Value::Array(
            s.status
                .iter()
                .map(|r| {
                    let mut item = Map::new();
                    item.insert("id".into(), Value::String(r.id.clone()));
                    item.insert("label".into(), Value::String(r.label.clone()));
                    item.insert("group".into(), Value::String(r.group.clone()));
                    item.insert("order".into(), Value::from(r.order));
                    item.insert("value".into(), Value::String(r.value.clone()));
                    item.insert("labelText".into(), plugin_message(r.label_text.as_ref()));
                    item.insert("groupLabel".into(), plugin_message(r.group_label.as_ref()));
                    // waywallen 0.3.8: САМО значение строки состояния тоже
                    // переводимо (страница «Источники» показывает его вместо
                    // `value`). Запаска — прежнее строковое поле.
                    item.insert(
                        "valueText".into(),
                        plugin_message_or(r.value_text.as_ref(), &r.value),
                    );
                    Value::Object(item)
                })
                .collect(),
        ),
    );
    Value::Object(map)
}

/// Источник, на котором открывается страница «Мастерская».
///
/// Берём то, что предложил демон, — как у вендора. Раньше мы подставляли
/// сюда Мастерскую Steam, потому что каталог отдавали сами и вход не
/// требовался; теперь каталог снова вендорский, и своего значения по
/// умолчанию у нас нет (см. `WORKSHOP_SERVED_LOCALLY` в шиме).
pub fn pick_default(_sources: &[Value], daemon_default: &str) -> String {
    daemon_default.to_string()
}

/// Состояние запроса доступности.
#[derive(Debug, Default)]
pub struct AvailabilityState {
    pub base: QueryState,
    pub sources: Vec<Value>,
    pub default_source_id: String,
    pending: BTreeMap<i64, ()>,
}

impl AvailabilityState {
    pub fn reload(&mut self, connected: bool) -> Vec<Cmd> {
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        let mut out = base_events(signals);
        if !send {
            return out;
        }
        let token = next_token();
        self.pending.insert(token, ());
        out.push(Cmd::Signal(Event::Sent {
            token,
            field: "remote_availability",
            request: Request::Availability,
        }));
        out
    }

    pub fn apply(&mut self, token: i64, frame: &[u8]) -> Vec<Cmd> {
        if self.pending.remove(&token).is_none() {
            // Ответ не наш (или уже обработан) — молча дропаем, как это
            // делал клиент по мёртвому владельцу.
            return Vec::new();
        }
        let (answer, mut out) = match take_response(&mut self.base, frame) {
            Ok(pair) => pair,
            Err(_) => return decode_failed(&mut self.base),
        };
        let Some(rsp) = answer else {
            return out;
        };
        let av = match rsp.payload {
            Some(pb::response::Payload::RemoteAvailability(av)) => av,
            _ => pb::RemoteAvailabilityResponse::default(),
        };
        let mut l10n = localizer().lock().expect("переводчик не отравлен");
        self.sources = av
            .sources
            .iter()
            .map(|s| {
                let info = source_info(s);
                l10n.translate_source(&info).unwrap_or(info)
            })
            .collect();
        drop(l10n);
        self.default_source_id = pick_default(&self.sources, &av.default_source_id);
        out.push(Cmd::Signal(Event::Prop("sourcesChanged")));
        out.extend(finish(&mut self.base));
        out
    }
}

/// Битый кадр: демон может быть новее нас, падать посреди приёма нельзя —
/// запрос уходит в Error с понятным текстом (в Python это ветка
/// `except` в `_core_call`).
fn decode_failed(state: &mut QueryState) -> Vec<Cmd> {
    let mut out = we_query::set_error(state, &rexon_core::i18n::tr("could not parse the daemon response"));
    out.extend(we_query::set_status(state, STATUS_ERROR));
    base_events(out)
}

// ============================================================================
// RemoteSearchQuery — выдача каталога с постраничной догрузкой
// ============================================================================

/// Что именно спрашивала страница — токен помнит это за замыкание `_done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingPage {
    page: u32,
    append: bool,
}

/// Состояние запроса поиска.
#[derive(Debug)]
pub struct SearchState {
    pub base: QueryState,
    pub source_id: String,
    pub query: String,
    pub sort_key: String,
    pub tags: Vec<String>,
    /// v0.2.6: страница гасит выдачу, пока источник требует входа.
    pub browsing_enabled: bool,
    /// QueryList (qextra): индекс последней загруженной страницы минус один.
    pub offset: i32,
    /// Выдача исчерпана.
    pub no_more: bool,
    /// Текст ошибки САМОГО источника (не транспорта) — показывается плашкой.
    pub error_text: String,
    pending: BTreeMap<i64, PendingPage>,
}

impl Default for SearchState {
    fn default() -> Self {
        SearchState {
            base: QueryState::default(),
            source_id: String::new(),
            query: String::new(),
            sort_key: String::new(),
            tags: Vec::new(),
            browsing_enabled: true,
            offset: 0,
            no_more: false,
            error_text: String::new(),
            pending: BTreeMap::new(),
        }
    }
}

impl SearchState {
    fn set_text(
        slot: &mut String,
        value: &str,
        signal: &'static str,
    ) -> Vec<Cmd> {
        if slot == value {
            return Vec::new();
        }
        *slot = value.to_string();
        vec![Cmd::Signal(Event::Prop(signal)), Cmd::RequestReload]
    }

    pub fn set_source_id(&mut self, value: &str) -> Vec<Cmd> {
        Self::set_text(&mut self.source_id, value, "sourceIdChanged")
    }

    pub fn set_query(&mut self, value: &str) -> Vec<Cmd> {
        Self::set_text(&mut self.query, value, "queryChanged")
    }

    pub fn set_sort_key(&mut self, value: &str) -> Vec<Cmd> {
        Self::set_text(&mut self.sort_key, value, "sortKeyChanged")
    }

    pub fn set_tags(&mut self, value: Vec<String>) -> Vec<Cmd> {
        if self.tags == value {
            return Vec::new();
        }
        self.tags = value;
        vec![Cmd::Signal(Event::Prop("tagsChanged")), Cmd::RequestReload]
    }

    pub fn set_browsing_enabled(&mut self, value: bool) -> Vec<Cmd> {
        if self.browsing_enabled == value {
            return Vec::new();
        }
        self.browsing_enabled = value;
        let mut out = vec![Cmd::Signal(Event::Prop("browsingEnabledChanged"))];
        if value {
            out.push(Cmd::RequestReload);
        } else {
            out.extend(self.clear_results());
        }
        out
    }

    /// Порт `clearResults`: страниц нет, догрузки нет, выдача пуста.
    pub fn clear_results(&mut self) -> Vec<Cmd> {
        self.offset = 0;
        self.no_more = true;
        self.error_text.clear();
        vec![
            Cmd::Model(ModelCmd::Reset {
                rows: Vec::new(),
                has_more: false,
            }),
            Cmd::Signal(Event::Prop("stateChanged")),
        ]
    }

    pub fn reload(&mut self, connected: bool) -> Vec<Cmd> {
        if !self.browsing_enabled {
            return Vec::new();
        }
        self.offset = 0;
        self.no_more = false;
        self.fetch_page(1, false, connected, true)
    }

    pub fn load_more(&mut self, connected: bool) -> Vec<Cmd> {
        if !self.browsing_enabled || self.no_more || self.base.status == we_query::STATUS_QUERYING
        {
            return Vec::new();
        }
        let page = (self.offset + 2) as u32;
        self.fetch_page(page, true, connected, true)
    }

    /// Догрузка по протоколу модели.
    ///
    /// `from_model = true` означает, что мы находимся ВНУТРИ обработчика
    /// `reqFetchMore` самой модели. Тогда трогать модель нельзя: она уже
    /// занята собой (в Rust это было бы вторым `&mut` на живой объект), а
    /// `setHasMore(false)` там всё равно пустышка — модель сбросила флаг
    /// строкой выше, перед тем как испустить сигнал. Наблюдаемая
    /// последовательность от этого не меняется, потому что пустая
    /// установка флага сигнала не даёт.
    pub fn fetch_more(&mut self, connected: bool, from_model: bool) -> Vec<Cmd> {
        if !self.browsing_enabled || self.no_more {
            return Vec::new();
        }
        let page = (self.offset + 2) as u32;
        self.fetch_page(page, true, connected, !from_model)
    }

    fn fetch_page(
        &mut self,
        page: u32,
        append: bool,
        connected: bool,
        touch_model: bool,
    ) -> Vec<Cmd> {
        let mut out = Vec::new();
        // Оригинал: t->setHasMore(false) ДО запроса — иначе GridView дёргает
        // fetchMore повторно, пока страница в полёте.
        if touch_model {
            out.push(Cmd::Model(ModelCmd::SetHasMore(false)));
        }
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        out.extend(base_events(signals));
        if !send {
            return out;
        }
        let token = next_token();
        self.pending.insert(token, PendingPage { page, append });
        // Подписи фильтров у нас русские — источнику уходят его же теги.
        let required_tags = localizer()
            .lock()
            .expect("переводчик не отравлен")
            .untranslate_tags(&self.tags, &self.source_id);
        out.push(Cmd::Signal(Event::Sent {
            token,
            field: "remote_search",
            request: Request::Search {
                source_id: self.source_id.clone(),
                query: self.query.clone(),
                sort_key: self.sort_key.clone(),
                page,
                required_tags,
            },
        }));
        out
    }

    pub fn apply(&mut self, token: i64, frame: &[u8]) -> Vec<Cmd> {
        let Some(pending) = self.pending.remove(&token) else {
            return Vec::new();
        };
        let (answer, mut out) = match take_response(&mut self.base, frame) {
            Ok(pair) => pair,
            Err(_) => return decode_failed(&mut self.base),
        };
        let Some(rsp) = answer else {
            return out;
        };
        let sr = match rsp.payload {
            Some(pb::response::Payload::RemoteSearch(sr)) => sr,
            _ => pb::RemoteSearchResponse::default(),
        };
        let rows: Vec<RemoteRow> = sr.items.iter().map(row_from_item).collect();
        // `has_more && rows` — так у моста: демон иногда обещает следующую
        // страницу, не прислав ни строки, и бесконечная прокрутка зациклилась
        // бы на пустых запросах.
        let more = sr.has_more && !rows.is_empty();
        out.extend(self.apply_page(rows, more, sr.error, pending.page, pending.append));
        out.extend(finish(&mut self.base));
        out
    }

    fn apply_page(
        &mut self,
        rows: Vec<RemoteRow>,
        more: bool,
        error: String,
        page: u32,
        append: bool,
    ) -> Vec<Cmd> {
        self.offset = page as i32 - 1;
        self.no_more = !more;
        self.error_text = error;
        vec![
            Cmd::Model(if append {
                ModelCmd::Append {
                    rows,
                    has_more: more,
                }
            } else {
                ModelCmd::Reset {
                    rows,
                    has_more: more,
                }
            }),
            Cmd::Signal(Event::Prop("stateChanged")),
        ]
    }
}

/// `RemoteItem` протокола → строка модели выдачи.
fn row_from_item(it: &pb::RemoteItem) -> RemoteRow {
    RemoteRow {
        item_id: it.id.clone(),
        source_id: it.source_id.clone(),
        title: it.title.clone(),
        preview_url: it.preview_url.clone(),
        author: it.author.clone(),
        wp_type: it.wp_type.clone(),
        // v0.2.6: булев downloaded → состояние приобретения (3 — есть).
        acquisition_state: if it.downloaded { 3 } else { 0 },
        // waywallen 0.5.0: аватар автора присылает сам источник
        // (`RemoteItem.author_avatar_url`, `remote_query.cpp`); пусто —
        // панель аватар прячет.
        author_avatar_url: it.author_avatar_url.clone(),
        // НАША ПРАВКА: страница работ автора — только у Мастерской Steam;
        // источники демона её не знают. Пусто прячет кнопку.
        author_url: String::new(),
    }
}

// ============================================================================
// RemoteDetailsQuery — панель деталей предмета
// ============================================================================

/// Состояние запроса деталей.
#[derive(Debug, Default)]
pub struct DetailsState {
    pub base: QueryState,
    pub source_id: String,
    pub item_id: String,
    /// Автор из ответа деталей (`RemoteDetailsResponse.author`, waywallen
    /// 0.4.4) — запасной, когда у строки выдачи автора нет.
    pub author: String,
    /// Аватар автора из ответа деталей (`RemoteDetailsResponse.author_avatar_url`,
    /// waywallen 0.5.0) — запасной, когда у строки выдачи аватара нет
    /// (`RemoteDetailPanel.qml`: `item?.authorAvatarUrl`, затем `details?.…`).
    pub author_avatar_url: String,
    pub description: String,
    pub size: String,
    pub width: i32,
    pub height: i32,
    pub tags: Vec<String>,
    /// ⚠️ Починка при порте (правило #16), сохранённая здесь: поле есть в
    /// протоколе и его ждёт вендорная панель («Открыть страницу в Steam»,
    /// RemoteDetailPanel.qml:58), а прежний мост его не отдавал — кнопка
    /// была скрыта всегда.
    pub web_url: String,
    pending: BTreeMap<i64, ()>,
}

impl DetailsState {
    pub fn set_source_id(&mut self, value: &str) -> Vec<Cmd> {
        if self.source_id == value {
            return Vec::new();
        }
        self.source_id = value.to_string();
        vec![
            Cmd::Signal(Event::Prop("sourceIdChanged")),
            Cmd::RequestReload,
        ]
    }

    pub fn set_item_id(&mut self, value: &str) -> Vec<Cmd> {
        if self.item_id == value {
            return Vec::new();
        }
        self.item_id = value.to_string();
        vec![Cmd::Signal(Event::Prop("itemIdChanged")), Cmd::RequestReload]
    }

    pub fn reload(&mut self, connected: bool) -> Vec<Cmd> {
        // Как в оригинале: сначала сброс полей, потом запрос. Панель не
        // должна секунду показывать описание ПРОШЛОГО предмета.
        self.author.clear();
        self.author_avatar_url.clear();
        self.description.clear();
        self.size.clear();
        self.width = 0;
        self.height = 0;
        self.tags.clear();
        self.web_url.clear();
        let mut out = vec![Cmd::Signal(Event::Prop("loaded"))];
        if self.item_id.is_empty() {
            return out;
        }
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        out.extend(base_events(signals));
        if !send {
            return out;
        }
        let token = next_token();
        self.pending.insert(token, ());
        out.push(Cmd::Signal(Event::Sent {
            token,
            field: "remote_details",
            request: Request::Details {
                source_id: self.source_id.clone(),
                id: self.item_id.clone(),
            },
        }));
        out
    }

    pub fn apply(&mut self, token: i64, frame: &[u8]) -> Vec<Cmd> {
        if self.pending.remove(&token).is_none() {
            return Vec::new();
        }
        let (answer, mut out) = match take_response(&mut self.base, frame) {
            Ok(pair) => pair,
            Err(_) => return decode_failed(&mut self.base),
        };
        let Some(rsp) = answer else {
            return out;
        };
        let dr = match rsp.payload {
            Some(pb::response::Payload::RemoteDetails(dr)) => dr,
            _ => pb::RemoteDetailsResponse::default(),
        };
        self.author = dr.author;
        self.author_avatar_url = dr.author_avatar_url;
        self.description = dr.description;
        self.size = dr.size;
        self.width = dr.width as i32;
        self.height = dr.height as i32;
        self.tags = rexon_core::remote_l10n::tag_labels(&dr.tags);
        self.web_url = dr.web_url;
        out.push(Cmd::Signal(Event::Prop("loaded")));
        out.extend(finish(&mut self.base));
        out
    }
}

// ============================================================================
// RemoteDownloadQuery — скачивание и удаление предмета
// ============================================================================

/// Что было заказано этим токеном.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingItem {
    Download { source_id: String, id: String },
    Uninstall { source_id: String, id: String },
}

/// Состояние запроса скачивания.
#[derive(Debug, Default)]
pub struct DownloadState {
    pub base: QueryState,
    pending: BTreeMap<i64, PendingItem>,
}

impl DownloadState {
    pub fn start(&mut self, source_id: &str, id: &str, connected: bool) -> Vec<Cmd> {
        if source_id.is_empty() || id.is_empty() {
            return Vec::new();
        }
        // Карточка сразу показывает «в очереди» — не дожидаясь демона.
        let mut out = vec![Cmd::Store {
            source_id: source_id.to_string(),
            id: id.to_string(),
            state: 1,
        }];
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        out.extend(base_events(signals));
        if send {
            let token = next_token();
            self.pending.insert(
                token,
                PendingItem::Download {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                },
            );
            out.push(Cmd::Signal(Event::Sent {
                token,
                field: "remote_download",
                request: Request::Download {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                },
            }));
        } else {
            // Кадр не ушёл — очередь снимается тут же, иначе карточка
            // навсегда осталась бы «в очереди» (у моста это done=_fail).
            out.extend(self.fail_queue(source_id, id));
        }
        out
    }

    pub fn remove(&mut self, source_id: &str, id: &str, connected: bool) -> Vec<Cmd> {
        if source_id.is_empty() || id.is_empty() {
            return Vec::new();
        }
        let (send, signals) = we_query::begin_call(&mut self.base, connected);
        let mut out = base_events(signals);
        if !send {
            return out;
        }
        let token = next_token();
        self.pending.insert(
            token,
            PendingItem::Uninstall {
                source_id: source_id.to_string(),
                id: id.to_string(),
            },
        );
        out.push(Cmd::Signal(Event::Sent {
            token,
            field: "remote_uninstall",
            request: Request::Uninstall {
                source_id: source_id.to_string(),
                id: id.to_string(),
            },
        }));
        out
    }

    /// Хвост скачивания: статус Error — снять очередь и объявить отказ.
    fn fail_queue(&self, source_id: &str, id: &str) -> Vec<Cmd> {
        if self.base.status != STATUS_ERROR {
            return Vec::new();
        }
        vec![
            Cmd::Store {
                source_id: source_id.to_string(),
                id: id.to_string(),
                state: 0,
            },
            Cmd::Signal(Event::Rejected {
                source_id: source_id.to_string(),
                id: id.to_string(),
                error: self.base.error.clone(),
            }),
        ]
    }

    pub fn apply(&mut self, token: i64, frame: &[u8]) -> Vec<Cmd> {
        let Some(pending) = self.pending.remove(&token) else {
            return Vec::new();
        };
        let (answer, mut out) = match take_response(&mut self.base, frame) {
            Ok(pair) => pair,
            Err(_) => return decode_failed(&mut self.base),
        };
        match pending {
            PendingItem::Download { source_id, id } => {
                if let Some(rsp) = answer {
                    let dr = match rsp.payload {
                        Some(pb::response::Payload::RemoteDownload(dr)) => dr,
                        _ => pb::RemoteDownloadResponse::default(),
                    };
                    if dr.accepted {
                        out.push(Cmd::Signal(Event::Accepted {
                            source_id: source_id.clone(),
                            id: id.clone(),
                        }));
                    } else {
                        out.push(Cmd::Store {
                            source_id: source_id.clone(),
                            id: id.clone(),
                            state: 0,
                        });
                        out.push(Cmd::Signal(Event::Rejected {
                            source_id: source_id.clone(),
                            id: id.clone(),
                            error: dr.error,
                        }));
                    }
                    out.extend(finish(&mut self.base));
                }
                // done=_fail в мосте зовётся ВСЕГДА, в том числе после
                // успешного разбора; работает он только при статусе Error.
                out.extend(self.fail_queue(&source_id, &id));
            }
            PendingItem::Uninstall { source_id, id } => {
                if let Some(rsp) = answer {
                    let ur = match rsp.payload {
                        Some(pb::response::Payload::RemoteUninstall(ur)) => ur,
                        _ => pb::RemoteUninstallResponse::default(),
                    };
                    if ur.removed {
                        out.push(Cmd::Store {
                            source_id: source_id.clone(),
                            id: id.clone(),
                            state: 0,
                        });
                        out.push(Cmd::Signal(Event::Removed {
                            source_id: source_id.clone(),
                            id: id.clone(),
                        }));
                    } else {
                        out.push(Cmd::Signal(Event::RemoveFailed {
                            source_id: source_id.clone(),
                            id: id.clone(),
                            error: ur.error,
                        }));
                    }
                    out.extend(finish(&mut self.base));
                }
            }
        }
        out
    }
}

// ============================================================================
// Мост Rust ↔ Qt: значения
// ============================================================================

fn qstringlist(values: &[String]) -> QStringList {
    QStringList::from_iter(values.iter().map(QString::from))
}

fn strings_of(list: &QStringList) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ============================================================================
// Qt-слой
// ============================================================================

/// Одинаковая для всех четырёх часть: свойства базы `Query`, рассылка её
/// сигналов и общий выключатель транспорта.
///
/// Это и есть обещанная планом замена Qt-наследования: макрос печатает то,
/// что у оригинала давал общий базовый класс.
macro_rules! impl_query_base {
    ($query:ident, $state:ident) => {
        impl qobject::$query {
            fn status(&self) -> i32 {
                self.$state.base.status
            }

            fn error(&self) -> QString {
                QString::from(&self.$state.base.error)
            }

            fn querying(&self) -> bool {
                self.$state.base.status == we_query::STATUS_QUERYING
            }

            fn forward_error(&self) -> bool {
                self.$state.base.forward_error
            }

            fn set_forward_error(mut self: Pin<&mut Self>, value: bool) {
                let cmds = set_forward_error_pure(
                    &mut self.as_mut().rust_mut().$state.base,
                    value,
                );
                self.run(&cmds);
            }

            fn set_transport_connected(&self, value: bool) {
                set_connected(value);
            }

            /// Разослать сигналы базы. Значения берутся ЖИВЫЕ (из состояния),
            /// а не запомненные при планировании: обработчик `onStatusChanged`
            /// в QML читает `status`/`error` и обязан увидеть новые значения.
            fn emit_signal(mut self: Pin<&mut Self>, event: &Event) -> bool {
                match event {
                    Event::Status(value) => {
                        self.as_mut().status_changed(*value);
                    }
                    Event::Querying => self.as_mut().querying_changed(),
                    Event::ErrorChanged => self.as_mut().error_changed(),
                    Event::ForwardErrorChanged => self.as_mut().forward_error_changed(),
                    Event::Finished => self.as_mut().finished(),
                    Event::ErrorOccurred(text) => {
                        self.as_mut().error_occurred(QString::from(text));
                    }
                    Event::AppError(text) => {
                        self.as_mut().app_error_occurred(QString::from(text));
                    }
                    Event::Sent {
                        token,
                        field,
                        request,
                    } => {
                        let bytes = request.encode(*token as u64);
                        // Владельца токена запоминаем ДО того, как кадр ушёл
                        // наружу: по нему шим вернёт ответ именно этому
                        // запросу, а не «тому, кто просил такое поле».
                        let owner = self_ptr(self.as_mut());
                        unsafe { we_transport::remember(*token, owner.cast()) };
                        self.as_mut().send_request(
                            *token,
                            QString::from(*field),
                            QByteArray::from(&bytes[..]),
                        );
                        // ⚠️ Колбэк обязан вернуться, НЕ доставив ответ: мы
                        // сейчас внутри &mut на этом же объекте, и повторный
                        // вход через deliverResponse взял бы вторую
                        // изменяемую ссылку. Питоновский шов только кладёт
                        // кадр в сокет.
                        we_transport::send(*token, field, &bytes);
                    }
                    // Не сигнал базы — разбирает вызывающий.
                    _ => return false,
                }
                true
            }
        }
    };
}

// ── RemoteAvailabilityQuery ─────────────────────────────────────────────────

/// Rust-состояние запроса доступности.
#[derive(Default)]
pub struct RemoteAvailabilityQueryRust {
    state: AvailabilityState,
    /// Таймер отложенной перезагрузки; владеет им Qt (ребёнок запроса).
    debounce: *mut qobject::QObject,
}

impl_query_base!(RemoteAvailabilityQuery, state);

impl cxx_qt::Initialize for qobject::RemoteAvailabilityQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        let timer = unsafe { qobject::rexon_new_reload_debounce(owner, DEBOUNCE_MS) };
        self.as_mut().rust_mut().debounce = timer;
    }
}

impl qobject::RemoteAvailabilityQuery {
    fn sources(&self) -> qobject::QList_QVariant {
        let mut list = QList::<QVariant>::default();
        for source in &self.state.sources {
            list.append(json_to_variant(source));
        }
        list
    }

    fn default_source_id(&self) -> QString {
        QString::from(&self.state.default_source_id)
    }

    fn reload(mut self: Pin<&mut Self>) {
        let cmds = self.as_mut().rust_mut().state.reload(connected());
        self.run(&cmds);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .apply(token, frame.as_slice());
        self.run(&cmds);
    }

    /// Исполнить список команд чистого слоя.
    fn run(mut self: Pin<&mut Self>, cmds: &[Cmd]) {
        for cmd in cmds {
            match cmd {
                Cmd::Signal(event) => {
                    if !self.as_mut().emit_signal(event) {
                        if let Event::Prop(name) = event {
                            debug_assert_eq!(*name, "sourcesChanged");
                            self.as_mut().sources_changed();
                        }
                    }
                }
                Cmd::RequestReload => {
                    let timer = self.debounce;
                    unsafe { qobject::rexon_restart_debounce(timer) };
                }
                // Ни модели, ни склада у этого запроса нет.
                Cmd::Model(_) | Cmd::Store { .. } => {}
            }
        }
    }
}

// ── RemoteSearchQuery ───────────────────────────────────────────────────────

/// Rust-состояние запроса поиска.
#[derive(Default)]
pub struct RemoteSearchQueryRust {
    state: SearchState,
    /// Модель выдачи: владеет ей Qt (модель — ребёнок запроса).
    model: *mut qobject::QObject,
    debounce: *mut qobject::QObject,
    /// Связь «модель просит страницу → запрос её просит». Guard обязан жить,
    /// пока жив запрос: его Drop разрывает соединение.
    fetch_guard: Option<cxx_qt::QMetaObjectConnectionGuard>,
}

impl_query_base!(RemoteSearchQuery, state);

/// Интервал дебаунса перезагрузки (qextra query.cpp:16).
const DEBOUNCE_MS: i32 = 100;

/// Указатель на владельца для замыкания связи «модель → запрос».
///
/// cxx-qt требует от обработчика сигнала `Send` (одна точка входа
/// обслуживает и очередные соединения), а сырой указатель им не является.
/// Наша связь ПРЯМАЯ и внутрисемейная: модель — ребёнок запроса, живут в
/// одном потоке, обработчик исполняется тем же потоком, что и `emit`.
/// Обёртка написана явно, чтобы это утверждение стояло в коде, а не
/// подразумевалось.
struct OwnerPtr(*mut qobject::RemoteSearchQuery);

// БЕЗОПАСНОСТЬ: указатель не пересекает границы потоков — см. выше.
unsafe impl Send for OwnerPtr {}

/// Указатель на сам объект как на QObject — им фабрика получает родителя.
fn self_ptr<T>(object: Pin<&mut T>) -> *mut qobject::QObject {
    let raw: *mut T = unsafe { object.get_unchecked_mut() };
    raw.cast()
}

impl cxx_qt::Initialize for qobject::RemoteSearchQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        // Модель создаётся в пост-конструкции, а не лениво в геттере: QML
        // читает свойство при первой же привязке, а геттер константный.
        let model = unsafe { qobject::rexon_new_search_result_model(owner) };
        let timer = unsafe { qobject::rexon_new_reload_debounce(owner, DEBOUNCE_MS) };
        {
            let mut rust = self.as_mut().rust_mut();
            rust.model = model;
            rust.debounce = timer;
        }

        // qextra try_connect_fetch_more: модель просит следующую страницу.
        // Указатель на владельца в замыкании безопасен ровно потому, что
        // модель — его ребёнок: она не переживёт запрос ни на такт.
        let query = OwnerPtr(unsafe { self.as_mut().get_unchecked_mut() });
        let model_ptr = model.cast::<crate::remote_list_model::qobject::RemoteListModel>();
        let guard = unsafe { Pin::new_unchecked(&mut *model_ptr) }.on_req_fetch_more(
            move |_model, _row| {
                // Захват ВСЕЙ обёртки, а не поля: в редакции 2021 замыкание
                // берёт `query.0` по отдельности — то есть голый указатель,
                // который не Send, и связь бы не собралась.
                let owner = &query;
                // ⚠️ Мы ВНУТРИ обработчика сигнала модели, то есть модель
                // сейчас занята собой. from_model=true запрещает её трогать.
                let query = unsafe { Pin::new_unchecked(&mut *owner.0) };
                query.fetch_more_internal(true);
            },
        );
        self.as_mut().rust_mut().fetch_guard = Some(guard);
    }
}

impl qobject::RemoteSearchQuery {
    fn model(&self) -> *mut qobject::QObject {
        self.model
    }

    fn source_id(&self) -> QString {
        QString::from(&self.state.source_id)
    }

    fn set_source_id(mut self: Pin<&mut Self>, value: QString) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .set_source_id(&value.to_string());
        self.run(&cmds);
    }

    fn query(&self) -> QString {
        QString::from(&self.state.query)
    }

    fn set_query(mut self: Pin<&mut Self>, value: QString) {
        let cmds = self.as_mut().rust_mut().state.set_query(&value.to_string());
        self.run(&cmds);
    }

    fn sort_key(&self) -> QString {
        QString::from(&self.state.sort_key)
    }

    fn set_sort_key(mut self: Pin<&mut Self>, value: QString) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .set_sort_key(&value.to_string());
        self.run(&cmds);
    }

    fn tags(&self) -> QStringList {
        qstringlist(&self.state.tags)
    }

    fn set_tags(mut self: Pin<&mut Self>, value: QStringList) {
        let cmds = self.as_mut().rust_mut().state.set_tags(strings_of(&value));
        self.run(&cmds);
    }

    fn browsing_enabled(&self) -> bool {
        self.state.browsing_enabled
    }

    fn set_browsing_enabled(mut self: Pin<&mut Self>, value: bool) {
        let cmds = self.as_mut().rust_mut().state.set_browsing_enabled(value);
        self.run(&cmds);
    }

    fn has_more(&self) -> bool {
        // v0.2.5: hasMore живёт в модели, запрос его только пересказывает.
        let model = self.model.cast::<crate::remote_list_model::qobject::RemoteListModel>();
        if model.is_null() {
            return false;
        }
        unsafe { (*model).has_more() }
    }

    fn error_text(&self) -> QString {
        QString::from(&self.state.error_text)
    }

    fn reload(mut self: Pin<&mut Self>) {
        let cmds = self.as_mut().rust_mut().state.reload(connected());
        self.run(&cmds);
    }

    fn load_more(mut self: Pin<&mut Self>) {
        let cmds = self.as_mut().rust_mut().state.load_more(connected());
        self.run(&cmds);
    }

    fn fetch_more(self: Pin<&mut Self>, _row: i32) {
        self.fetch_more_internal(false);
    }

    /// Общая часть догрузки; `from_model` — см. `SearchState::fetch_more`.
    fn fetch_more_internal(mut self: Pin<&mut Self>, from_model: bool) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .fetch_more(connected(), from_model);
        self.run(&cmds);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .apply(token, frame.as_slice());
        self.run(&cmds);
    }

    fn run(mut self: Pin<&mut Self>, cmds: &[Cmd]) {
        for cmd in cmds {
            match cmd {
                Cmd::Signal(event) => {
                    if !self.as_mut().emit_signal(event) {
                        if let Event::Prop(name) = event {
                            match *name {
                                "sourceIdChanged" => self.as_mut().source_id_changed(),
                                "queryChanged" => self.as_mut().query_changed(),
                                "sortKeyChanged" => self.as_mut().sort_key_changed(),
                                "tagsChanged" => self.as_mut().tags_changed(),
                                "browsingEnabledChanged" => {
                                    self.as_mut().browsing_enabled_changed()
                                }
                                "stateChanged" => self.as_mut().state_changed(),
                                other => debug_assert!(false, "нет сигнала {other}"),
                            }
                        }
                    }
                }
                Cmd::Model(model_cmd) => {
                    let model = self
                        .model
                        .cast::<crate::remote_list_model::qobject::RemoteListModel>();
                    if model.is_null() {
                        continue;
                    }
                    // Модель — отдельный QObject со своим состоянием; сюда
                    // приходит только команда, все сигналы она шлёт сама.
                    let model = unsafe { Pin::new_unchecked(&mut *model) };
                    match model_cmd {
                        ModelCmd::SetHasMore(value) => model.set_page_has_more(*value),
                        ModelCmd::Reset { rows, has_more } => {
                            model.reset_page(rows.clone(), *has_more)
                        }
                        ModelCmd::Append { rows, has_more } => {
                            model.append_page(rows.clone(), *has_more)
                        }
                    }
                }
                Cmd::RequestReload => {
                    let timer = self.debounce;
                    unsafe { qobject::rexon_restart_debounce(timer) };
                }
                Cmd::Store { .. } => {}
            }
        }
    }
}

// ── RemoteDetailsQuery ──────────────────────────────────────────────────────

/// Rust-состояние запроса деталей.
#[derive(Default)]
pub struct RemoteDetailsQueryRust {
    state: DetailsState,
    debounce: *mut qobject::QObject,
}

impl_query_base!(RemoteDetailsQuery, state);

impl cxx_qt::Initialize for qobject::RemoteDetailsQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        let timer = unsafe { qobject::rexon_new_reload_debounce(owner, DEBOUNCE_MS) };
        self.as_mut().rust_mut().debounce = timer;
    }
}

impl qobject::RemoteDetailsQuery {
    fn source_id(&self) -> QString {
        QString::from(&self.state.source_id)
    }

    fn set_source_id(mut self: Pin<&mut Self>, value: QString) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .set_source_id(&value.to_string());
        self.run(&cmds);
    }

    fn item_id(&self) -> QString {
        QString::from(&self.state.item_id)
    }

    fn set_item_id(mut self: Pin<&mut Self>, value: QString) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .set_item_id(&value.to_string());
        self.run(&cmds);
    }

    fn author(&self) -> QString {
        QString::from(&self.state.author)
    }

    fn author_avatar_url(&self) -> QString {
        QString::from(&self.state.author_avatar_url)
    }

    fn description(&self) -> QString {
        QString::from(&self.state.description)
    }

    fn size(&self) -> QString {
        QString::from(&self.state.size)
    }

    fn width(&self) -> i32 {
        self.state.width
    }

    fn height(&self) -> i32 {
        self.state.height
    }

    fn tags(&self) -> QStringList {
        qstringlist(&self.state.tags)
    }

    fn web_url(&self) -> QString {
        QString::from(&self.state.web_url)
    }

    fn reload(mut self: Pin<&mut Self>) {
        let cmds = self.as_mut().rust_mut().state.reload(connected());
        self.run(&cmds);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .apply(token, frame.as_slice());
        self.run(&cmds);
    }

    fn run(mut self: Pin<&mut Self>, cmds: &[Cmd]) {
        for cmd in cmds {
            match cmd {
                Cmd::Signal(event) => {
                    if !self.as_mut().emit_signal(event) {
                        if let Event::Prop(name) = event {
                            match *name {
                                "sourceIdChanged" => self.as_mut().source_id_changed(),
                                "itemIdChanged" => self.as_mut().item_id_changed(),
                                "loaded" => self.as_mut().loaded(),
                                other => debug_assert!(false, "нет сигнала {other}"),
                            }
                        }
                    }
                }
                Cmd::RequestReload => {
                    let timer = self.debounce;
                    unsafe { qobject::rexon_restart_debounce(timer) };
                }
                Cmd::Model(_) | Cmd::Store { .. } => {}
            }
        }
    }
}

// ── RemoteDownloadQuery ─────────────────────────────────────────────────────

/// Rust-состояние запроса скачивания.
#[derive(Default)]
pub struct RemoteDownloadQueryRust {
    state: DownloadState,
}

impl_query_base!(RemoteDownloadQuery, state);

impl cxx_qt::Initialize for qobject::RemoteDownloadQuery {
    fn initialize(self: Pin<&mut Self>) {
        // Ни модели, ни дебаунса: запрос-действие, у него нет reload().
    }
}

impl qobject::RemoteDownloadQuery {
    fn start(mut self: Pin<&mut Self>, source_id: &QString, id: &QString) {
        let cmds = self.as_mut().rust_mut().state.start(
            &source_id.to_string(),
            &id.to_string(),
            connected(),
        );
        self.run(&cmds);
    }

    fn remove(mut self: Pin<&mut Self>, source_id: &QString, id: &QString) {
        let cmds = self.as_mut().rust_mut().state.remove(
            &source_id.to_string(),
            &id.to_string(),
            connected(),
        );
        self.run(&cmds);
    }

    fn uninstall(self: Pin<&mut Self>, source_id: &QString, id: &QString) {
        self.remove(source_id, id);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let cmds = self
            .as_mut()
            .rust_mut()
            .state
            .apply(token, frame.as_slice());
        self.run(&cmds);
    }

    fn run(mut self: Pin<&mut Self>, cmds: &[Cmd]) {
        for cmd in cmds {
            match cmd {
                Cmd::Signal(event) => {
                    if self.as_mut().emit_signal(event) {
                        continue;
                    }
                    match event {
                        Event::Accepted { source_id, id } => {
                            self.as_mut()
                                .accepted(QString::from(source_id), QString::from(id));
                        }
                        Event::Rejected {
                            source_id,
                            id,
                            error,
                        } => {
                            self.as_mut().rejected(
                                QString::from(source_id),
                                QString::from(id),
                                QString::from(error),
                            );
                        }
                        Event::Removed { source_id, id } => {
                            self.as_mut()
                                .removed(QString::from(source_id), QString::from(id));
                        }
                        Event::RemoveFailed {
                            source_id,
                            id,
                            error,
                        } => {
                            self.as_mut().remove_failed(
                                QString::from(source_id),
                                QString::from(id),
                                QString::from(error),
                            );
                        }
                        other => debug_assert!(false, "нет сигнала для {other:?}"),
                    }
                }
                Cmd::Store {
                    source_id,
                    id,
                    state,
                } => {
                    // Общий склад: состояние приобретения уходит во ВСЕ живые
                    // модели выдачи сразу — и в строку списка, и в панель
                    // деталей, поэтому кнопка меняется без перезагрузки.
                    acquisition::set(source_id, id, *state);
                }
                Cmd::Model(_) | Cmd::RequestReload => {}
            }
        }
    }
}

// ============================================================================
// Голден: эталон, снятый с ЖИВЫХ Python-запросов
// ============================================================================

/// Разбор и воспроизведение `tests/golden/discover_queries_cases.json`.
///
/// Живёт не под `#[cfg(test)]`, потому что тем же эталоном пользуется стенд
/// `selftest::run()`: чистый слой проверяется `cargo test`, а Qt-слой —
/// настоящими QObject'ами.
pub mod golden {
    use super::{Cmd, Event, ModelCmd, Request};
    use crate::remote_list_model::{ModelEvent, RemoteRow, RemoteRows, ROLE_NAMES};
    use prost::Message;
    use rexon_core::proto::control as pb;
    use serde_json::Value;
    use std::collections::BTreeMap;

    /// Голден в исходниках крейта — тот самый файл, который печатает
    /// tests/golden/discover_queries_cases.py с живого Python-моста.
    pub const GOLDEN_JSON: &str = include_str!("../tests/golden/discover_queries_cases.json");

    /// Копия описаний источников без карт `PluginMessage` и с вариантами
    /// фильтра, сложенными обратно в плоский список подписей.
    ///
    /// Ключи с суффиксом `Text` и `groupLabel` — это подписи waywallen 0.3.7,
    /// которых нет в эталоне с Python-моста. Снимаем их рекурсивно, чтобы
    /// сравнить ровно то, что сравнивалось раньше. Тем же приёмом снимается и
    /// перемена 0.3.8: `values: ["Аниме"]` стал
    /// `options: [{value, label, labelText}]`, и в эталоне Python-эпохи ему
    /// соответствует список ПОДПИСЕЙ.
    ///
    /// ⭐ Лежит в `golden`, а не под `#[cfg(test)]`, по той же причине, что и
    /// весь модуль: ровно эта сверка нужна и Qt-стенду `selftest::run()`.
    /// Пока функция была видна одному `cargo test`, стенд сравнивал живой
    /// ответ 0.3.8 с эталоном Python-эпохи напрямую — и краснел не на
    /// расхождении поведения, а на новых полях протокола.
    pub fn strip_message_maps(value: &Value) -> Value {
        if let Value::Object(map) = value {
            if let Some(Value::Array(options)) = map.get("options") {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    if k == "options" {
                        out.insert(
                            "values".into(),
                            Value::Array(
                                options
                                    .iter()
                                    .map(|o| o.get("label").cloned().unwrap_or(Value::Null))
                                    .collect(),
                            ),
                        );
                        continue;
                    }
                    out.insert(k.clone(), strip_message_maps(v));
                }
                out.retain(|k, _| !k.ends_with("Text") && k.as_str() != "groupLabel");
                return Value::Object(out);
            }
        }
        match value {
            Value::Array(items) => Value::Array(items.iter().map(strip_message_maps).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(k, _)| !k.ends_with("Text") && k.as_str() != "groupLabel")
                    .map(|(k, v)| (k.clone(), strip_message_maps(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Чего не хватает картам `PluginMessage` в описаниях источников.
    ///
    /// Пустой список — всё на месте. Без этой половины сверки первая
    /// (`strip_message_maps`) молча простила бы ПОЛНУЮ пропажу новых полей:
    /// она их именно что снимает.
    ///
    /// Возвращает список, а не паникует, потому что зовущих двое: `cargo
    /// test` делает из него `assert!`, стенд — строки приговора.
    pub fn message_map_problems(sources: &Value) -> Vec<String> {
        let mut problems = Vec::new();
        let mut seen = 0usize;
        for source in sources.as_array().into_iter().flatten() {
            let Some(map) = source.as_object() else { continue };
            for key in ["displayNameText", "remoteHintText"] {
                match map.get(key) {
                    None => problems.push(format!("у источника нет карты {key}")),
                    Some(message) => {
                        if message.get("pluginId").is_none() || message.get("msgid").is_none() {
                            problems
                                .push(format!("карта {key} без обязательных ключей: {message}"));
                        }
                        seen += 1;
                    }
                }
            }
            for sort in map.get("sorts").into_iter().flat_map(|v| v.as_array()).flatten() {
                if sort.get("labelText").is_none() {
                    problems.push("у сортировки нет карты labelText".to_string());
                }
                seen += 1;
            }
            // waywallen 0.3.8: у варианта фильтра появились подпись и её
            // переводимая пара. Складывающая половина сверки
            // (`strip_message_maps`) смотрит только на `label`, поэтому
            // пропажу `value`/`labelText` ловим здесь.
            for filter in map.get("filters").into_iter().flat_map(|v| v.as_array()).flatten() {
                for option in filter
                    .get("options")
                    .into_iter()
                    .flat_map(|v| v.as_array())
                    .flatten()
                {
                    if option.get("value").is_none()
                        || option.get("label").is_none()
                        || option.get("labelText").is_none()
                    {
                        problems.push(format!(
                            "вариант фильтра без value/label/labelText: {option}"
                        ));
                    }
                    seen += 1;
                }
            }
            // ⭐ Значение строки состояния — подпись для человека, поэтому
            // переведено; машинный ключ обязан остаться рядом в `valueText`.
            // Без этой проверки перевод `value` было бы не отличить от
            // потери ключа, по которому страница «Источники» и переводит.
            for status in map.get("status").into_iter().flat_map(|v| v.as_array()).flatten() {
                if status.get("valueText").is_none() {
                    problems.push(format!(
                        "у строки состояния нет ключа valueText: {status}"
                    ));
                }
                seen += 1;
            }
        }
        // Случай «демон ответил ошибкой» источников не приносит вовсе —
        // требовать карты там не с чего.
        let empty = sources.as_array().is_none_or(|items| items.is_empty());
        if !empty && seen == 0 {
            problems.push("источники есть, а карт подписей нет вовсе".to_string());
        }
        problems
    }

    /// Сколько описаний источников принёс случай: стенду нужно доказать, что
    /// сверке было что сравнивать, а не что она прошла на пустом списке.
    pub fn source_count(sources: &Value) -> usize {
        sources.as_array().map_or(0, Vec::len)
    }

    pub fn load() -> Value {
        serde_json::from_str(GOLDEN_JSON).expect("голден читается")
    }

    /// Наблюдаемая запись журнала: кто и что испустил.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Record {
        /// Имя объекта: availability / search / details / download /
        /// search.model.
        pub q: String,
        pub kind: RecordKind,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RecordKind {
        Query(Event),
        Model(ModelEvent),
    }

    pub fn str_at(value: &Value, key: &str) -> String {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn i64_at(value: &Value, key: &str) -> i64 {
        value.get(key).and_then(Value::as_i64).unwrap_or_default()
    }

    pub fn bool_at(value: &Value, key: &str) -> bool {
        value.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn strings_at(value: &Value, key: &str) -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn array_at<'a>(value: &'a Value, key: &str) -> &'a [Value] {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[])
    }

    /// Кадр запроса из голдена (поля печатались по описанию protobuf).
    fn request_of(field: &str, request: &Value) -> Request {
        match field {
            "remote_availability" => Request::Availability,
            "remote_search" => Request::Search {
                source_id: str_at(request, "source_id"),
                query: str_at(request, "query"),
                sort_key: str_at(request, "sort_key"),
                page: i64_at(request, "page") as u32,
                required_tags: strings_at(request, "required_tags"),
            },
            "remote_details" => Request::Details {
                source_id: str_at(request, "source_id"),
                id: str_at(request, "id"),
            },
            "remote_download" => Request::Download {
                source_id: str_at(request, "source_id"),
                id: str_at(request, "id"),
            },
            "remote_uninstall" => Request::Uninstall {
                source_id: str_at(request, "source_id"),
                id: str_at(request, "id"),
            },
            other => panic!("голден: неизвестное поле запроса {other}"),
        }
    }

    /// Имена сигналов запросов, которые в порте выражены Event::Prop.
    /// Список закрытый: незнакомое имя роняет разбор, а не молча теряется.
    fn prop_of(sig: &str) -> &'static str {
        match sig {
            "sourcesChanged" => "sourcesChanged",
            "sourceIdChanged" => "sourceIdChanged",
            "queryChanged" => "queryChanged",
            "sortKeyChanged" => "sortKeyChanged",
            "tagsChanged" => "tagsChanged",
            "browsingEnabledChanged" => "browsingEnabledChanged",
            "stateChanged" => "stateChanged",
            "itemIdChanged" => "itemIdChanged",
            "loaded" => "loaded",
            other => panic!("голден: неизвестный сигнал запроса {other}"),
        }
    }

    /// Ожидаемый журнал случая: голден → записи порта.
    ///
    /// Глобальная пересылка ошибки в голдене помечена `q = "app"` (её шлёт
    /// синглтон App). В порте App ещё нет — её шлёт сам запрос сигналом
    /// `appErrorOccurred`, поэтому запись приписывается тому запросу, чей
    /// `errorOccurred` только что прошёл. Порядок этим не меняется: у
    /// оригинала пересылка идёт сразу за собственным сигналом.
    pub fn expected_records(case: &Value) -> Vec<Record> {
        let mut out = Vec::new();
        let mut last_error_owner = String::new();
        for event in case
            .get("events")
            .and_then(Value::as_array)
            .expect("голден: у случая нет событий")
        {
            let q = str_at(event, "q");
            let sig = str_at(event, "sig");
            if q == "app" {
                assert_eq!(sig, "errorOccurred", "голден: у App иной сигнал");
                out.push(Record {
                    q: last_error_owner.clone(),
                    kind: RecordKind::Query(Event::AppError(str_at(event, "error"))),
                });
                continue;
            }
            if q.ends_with(".model") {
                let kind = match sig.as_str() {
                    "modelReset" => ModelEvent::ModelReset,
                    "rowsInserted" => ModelEvent::RowsInserted {
                        first: i64_at(event, "first") as usize,
                        last: i64_at(event, "last") as usize,
                    },
                    "countChanged" => ModelEvent::CountChanged,
                    "hasMoreChanged" => ModelEvent::HasMoreChanged(bool_at(event, "value")),
                    "reqFetchMore" => ModelEvent::ReqFetchMore(i64_at(event, "row") as usize),
                    "dataChanged" => {
                        let roles = strings_at(event, "roles");
                        assert_eq!(
                            roles.len(),
                            1,
                            "голден: dataChanged обязан нести РОВНО одну роль, а несёт {roles:?}"
                        );
                        let role = ROLE_NAMES
                            .iter()
                            .find(|n| **n == roles[0])
                            .unwrap_or_else(|| panic!("голден: неизвестная роль {}", roles[0]));
                        ModelEvent::DataChanged {
                            first: i64_at(event, "first") as usize,
                            last: i64_at(event, "last") as usize,
                            role,
                        }
                    }
                    other => panic!("голден: неизвестный сигнал модели {other}"),
                };
                out.push(Record {
                    q,
                    kind: RecordKind::Model(kind),
                });
                continue;
            }
            let kind = match sig.as_str() {
                "statusChanged" => Event::Status(i64_at(event, "status") as i32),
                "queryingChanged" => Event::Querying,
                "errorChanged" => Event::ErrorChanged,
                "forwardErrorChanged" => Event::ForwardErrorChanged,
                "finished" => Event::Finished,
                "errorOccurred" => {
                    last_error_owner = q.clone();
                    Event::ErrorOccurred(str_at(event, "error"))
                }
                "sent" => {
                    let field = str_at(event, "field");
                    let request = request_of(&field, event.get("request").unwrap_or(&Value::Null));
                    Event::Sent {
                        token: i64_at(event, "token"),
                        field: request.field(),
                        request,
                    }
                }
                "accepted" => Event::Accepted {
                    source_id: str_at(event, "sourceId"),
                    id: str_at(event, "itemId"),
                },
                "rejected" => Event::Rejected {
                    source_id: str_at(event, "sourceId"),
                    id: str_at(event, "itemId"),
                    error: str_at(event, "error"),
                },
                "removed" => Event::Removed {
                    source_id: str_at(event, "sourceId"),
                    id: str_at(event, "itemId"),
                },
                "removeFailed" => Event::RemoveFailed {
                    source_id: str_at(event, "sourceId"),
                    id: str_at(event, "itemId"),
                    error: str_at(event, "error"),
                },
                other => Event::Prop(prop_of(other)),
            };
            out.push(Record {
                q,
                kind: RecordKind::Query(kind),
            });
        }
        out
    }

    /// Описание ответа из сценария → настоящий кадр `ServerFrame`.
    ///
    /// Зеркало `build_response` генератора: обе стороны собирают ОДИН и тот
    /// же protobuf из одного описания, поэтому эталон не зависит от того,
    /// кто его читает.
    pub fn build_frame(step: &Value) -> Vec<u8> {
        let mut rsp = pb::Response {
            status: pb::Status::Ok as i32,
            ..Default::default()
        };
        if let Some(status) = step.get("status").and_then(Value::as_str) {
            rsp.status = match status {
                "OK" => pb::Status::Ok as i32,
                "INVALID_ARGUMENT" => pb::Status::InvalidArgument as i32,
                "NOT_FOUND" => pb::Status::NotFound as i32,
                "INTERNAL" => pb::Status::Internal as i32,
                "FAILED_PRECONDITION" => pb::Status::FailedPrecondition as i32,
                other => panic!("голден: неизвестный статус ответа {other}"),
            };
            rsp.message = str_at(step, "message");
        }
        if let Some(payload) = step.get("payload") {
            rsp.payload = Some(payload_of(payload));
        }
        pb::ServerFrame {
            kind: Some(pb::server_frame::Kind::Response(rsp)),
        }
        .encode_to_vec()
    }

    fn payload_of(payload: &Value) -> pb::response::Payload {
        match str_at(payload, "kind").as_str() {
            "remote_availability" => {
                let mut av = pb::RemoteAvailabilityResponse {
                    default_source_id: str_at(payload, "defaultSourceId"),
                    ..Default::default()
                };
                for s in array_at(payload, "sources") {
                    av.sources.push(pb::RemoteSourceInfo {
                        id: str_at(s, "id"),
                        name: str_at(s, "name"),
                        display_name: str_at(s, "displayName"),
                        supports_search: bool_at(s, "supportsSearch"),
                        content_dir: str_at(s, "contentDir"),
                        owner_plugin_id: str_at(s, "ownerPluginId"),
                        remote_capability: i64_at(s, "remoteCapability") as i32,
                        remote_hint: str_at(s, "remoteHint"),
                        avatar_url: str_at(s, "avatarUrl"),
                        tags: strings_at(s, "tags"),
                        sorts: array_at(s, "sorts")
                            .iter()
                            .map(|o| pb::RemoteSortOption {
                                key: str_at(o, "key"),
                                label: str_at(o, "label"),
                                ..Default::default()
                            })
                            .collect(),
                        filters: array_at(s, "filters")
                            .iter()
                            .map(|f| pb::RemoteFilterDef {
                                id: str_at(f, "id"),
                                title: str_at(f, "title"),
                                r#type: i64_at(f, "type") as i32,
                                // Эталон снят с Python-моста, который знал
                                // ещё плоский `values` (waywallen ≤0.3.7).
                                // Здесь он раскладывается в варианты 0.3.8:
                                // значение = подпись, подписи плагина нет.
                                options: strings_at(f, "values")
                                    .into_iter()
                                    .map(|v| pb::RemoteFilterOption {
                                        value: v,
                                        ..Default::default()
                                    })
                                    .collect(),
                                description: str_at(f, "description"),
                                confirmation: str_at(f, "confirmation"),
                                ..Default::default()
                            })
                            .collect(),
                        actions: array_at(s, "actions")
                            .iter()
                            .map(|a| pb::PluginActionDef {
                                id: str_at(a, "id"),
                                label: str_at(a, "label"),
                                group: str_at(a, "group"),
                                order: i64_at(a, "order") as i32,
                                kind: i64_at(a, "kind") as i32,
                                visible: bool_at(a, "visible"),
                                enabled: bool_at(a, "enabled"),
                                description: str_at(a, "description"),
                                required_for_browsing: bool_at(a, "requiredForBrowsing"),
                                browse_description: str_at(a, "browseDescription"),
                                browse_button_label: str_at(a, "browseButtonLabel"),
                                fields: array_at(a, "fields")
                                    .iter()
                                    .map(|f| pb::PluginActionField {
                                        key: str_at(f, "key"),
                                        label: str_at(f, "label"),
                                        description: str_at(f, "description"),
                                        placeholder: str_at(f, "placeholder"),
                                        secret: bool_at(f, "secret"),
                                        required: bool_at(f, "required"),
                                        ..Default::default()
                                    })
                                    .collect(),
                                ..Default::default()
                            })
                            .collect(),
                        status: array_at(s, "status")
                            .iter()
                            .map(|r| pb::PluginStatusRow {
                                id: str_at(r, "id"),
                                label: str_at(r, "label"),
                                group: str_at(r, "group"),
                                order: i64_at(r, "order") as i32,
                                value: str_at(r, "value"),
                                ..Default::default()
                            })
                            .collect(),
                        settings: Vec::new(),
                        ..Default::default()
                    });
                }
                pb::response::Payload::RemoteAvailability(av)
            }
            "remote_search" => pb::response::Payload::RemoteSearch(pb::RemoteSearchResponse {
                has_more: bool_at(payload, "hasMore"),
                error: str_at(payload, "error"),
                items: array_at(payload, "items")
                    .iter()
                    .map(|it| pb::RemoteItem {
                        id: str_at(it, "id"),
                        title: str_at(it, "title"),
                        preview_url: str_at(it, "previewUrl"),
                        author: str_at(it, "author"),
                        downloaded: bool_at(it, "downloaded"),
                        source_id: str_at(it, "sourceId"),
                        wp_type: str_at(it, "wpType"),
                        author_avatar_url: str_at(it, "authorAvatarUrl"),
                    })
                    .collect(),
            }),
            "remote_details" => pb::response::Payload::RemoteDetails(pb::RemoteDetailsResponse {
                description: str_at(payload, "description"),
                size: str_at(payload, "size"),
                tags: strings_at(payload, "tags"),
                error: str_at(payload, "error"),
                width: i64_at(payload, "width") as u32,
                height: i64_at(payload, "height") as u32,
                web_url: str_at(payload, "webUrl"),
                author: str_at(payload, "author"),
                author_avatar_url: str_at(payload, "authorAvatarUrl"),
            }),
            "remote_download" => {
                pb::response::Payload::RemoteDownload(pb::RemoteDownloadResponse {
                    accepted: bool_at(payload, "accepted"),
                    error: str_at(payload, "error"),
                })
            }
            "remote_uninstall" => {
                pb::response::Payload::RemoteUninstall(pb::RemoteUninstallResponse {
                    removed: bool_at(payload, "removed"),
                    error: str_at(payload, "error"),
                })
            }
            other => panic!("голден: неизвестная полезная нагрузка {other}"),
        }
    }

    // ── Воспроизведение случая ЧИСТЫМ слоем ──────────────────────────────

    /// Стенд чистого слоя: четыре состояния запросов и одна модель выдачи.
    ///
    /// Общий склад состояний приобретения здесь СИНХРОННЫЙ, как в Python
    /// (прямое соединение сигнала). В живом Qt-слое он идёт очередью цикла
    /// событий — это осознанная разница, и её последствия разбирает стенд
    /// `selftest`, а не этот прогон.
    #[derive(Default)]
    pub struct Harness {
        pub availability: super::AvailabilityState,
        pub search: super::SearchState,
        pub details: super::DetailsState,
        pub download: super::DownloadState,
        pub rows: RemoteRows,
        pub journal: Vec<Record>,
        /// Значения `errorText` в моменты `stateChanged` — то, что реально
        /// видел покупатель (на этом сигнале висит notify свойства).
        pub error_text_trace: Vec<String>,
        /// То же для `error` базы: у поиска оба свойства в мосте живут в
        /// одном атрибуте и затирают друг друга.
        pub error_trace: Vec<String>,
        token_owner: BTreeMap<i64, String>,
        pending_reload: Vec<String>,
    }

    impl Harness {
        fn record_query(&mut self, q: &str, event: Event) {
            if let Event::Sent { token, .. } = &event {
                self.token_owner.insert(*token, q.to_string());
            }
            if q == "search" && event == Event::Prop("stateChanged") {
                self.error_text_trace.push(self.search.error_text.clone());
            }
            if q == "search" && event == Event::ErrorChanged {
                self.error_trace.push(self.search.base.error.clone());
            }
            self.journal.push(Record {
                q: q.to_string(),
                kind: RecordKind::Query(event),
            });
        }

        fn record_model(&mut self, events: Vec<ModelEvent>) {
            for event in events {
                self.journal.push(Record {
                    q: "search.model".to_string(),
                    kind: RecordKind::Model(event),
                });
            }
        }

        fn run(&mut self, q: &str, cmds: Vec<Cmd>) {
            for cmd in cmds {
                match cmd {
                    Cmd::Signal(event) => self.record_query(q, event),
                    Cmd::Model(model) => {
                        let events = match model {
                            ModelCmd::SetHasMore(value) => self.rows.set_has_more(value),
                            ModelCmd::Reset { rows, has_more } => {
                                self.rows.reset_rows(rows, has_more)
                            }
                            ModelCmd::Append { rows, has_more } => {
                                self.rows.append_rows(rows, has_more)
                            }
                        };
                        self.record_model(events);
                    }
                    Cmd::Store {
                        source_id,
                        id,
                        state,
                    } => {
                        let events = self.rows.set_acquisition_state(&source_id, &id, state);
                        self.record_model(events);
                    }
                    Cmd::RequestReload => {
                        if !self.pending_reload.iter().any(|p| p == q) {
                            self.pending_reload.push(q.to_string());
                        }
                    }
                }
            }
        }

        /// Сработал дебаунс: запросы, у которых менялись параметры,
        /// перезагружаются — по одному разу, сколько бы правок ни было.
        fn settle(&mut self) {
            let pending = std::mem::take(&mut self.pending_reload);
            for q in pending {
                let cmds = match q.as_str() {
                    "availability" => self.availability.reload(super::connected()),
                    "search" => self.search.reload(super::connected()),
                    "details" => self.details.reload(super::connected()),
                    other => panic!("голден: у {other} нет отложенной перезагрузки"),
                };
                self.run(&q, cmds);
            }
        }

        /// Так спрашивает GridView: через протокол самой модели.
        ///
        /// Порядок записей повторяет порядок слотов Qt: запрос подключён к
        /// `reqFetchMore` в конструкторе, то есть РАНЬШЕ журнала, поэтому
        /// его работа попадает в журнал перед самим `reqFetchMore`.
        fn model_fetch_more(&mut self) {
            for event in self.rows.fetch_more(true) {
                if matches!(event, ModelEvent::ReqFetchMore(_)) {
                    let cmds = self.search.fetch_more(super::connected(), true);
                    self.run("search", cmds);
                }
                self.record_model(vec![event]);
            }
        }

        pub fn step(&mut self, step: &Value) {
            match str_at(step, "op").as_str() {
                "set" => {
                    let q = str_at(step, "q");
                    let name = str_at(step, "name");
                    let value = step.get("value").cloned().unwrap_or(Value::Null);
                    let cmds = match (q.as_str(), name.as_str()) {
                        ("search", "sourceId") => {
                            self.search.set_source_id(value.as_str().unwrap_or_default())
                        }
                        ("search", "query") => {
                            self.search.set_query(value.as_str().unwrap_or_default())
                        }
                        ("search", "sortKey") => {
                            self.search.set_sort_key(value.as_str().unwrap_or_default())
                        }
                        ("search", "tags") => self.search.set_tags(
                            value
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .map(|v| v.as_str().unwrap_or_default().to_string())
                                        .collect()
                                })
                                .unwrap_or_default(),
                        ),
                        ("search", "browsingEnabled") => self
                            .search
                            .set_browsing_enabled(value.as_bool().unwrap_or(false)),
                        ("details", "sourceId") => self
                            .details
                            .set_source_id(value.as_str().unwrap_or_default()),
                        ("details", "itemId") => {
                            self.details.set_item_id(value.as_str().unwrap_or_default())
                        }
                        ("availability", "forwardError") => super::set_forward_error_pure(
                            &mut self.availability.base,
                            value.as_bool().unwrap_or(false),
                        ),
                        (other_q, other_name) => {
                            panic!("голден: нет свойства {other_q}.{other_name}")
                        }
                    };
                    self.run(&q, cmds);
                }
                "call" => {
                    let q = str_at(step, "q");
                    let method = str_at(step, "method");
                    let cmds = match (q.as_str(), method.as_str()) {
                        ("availability", "reload") => self.availability.reload(super::connected()),
                        ("search", "reload") => self.search.reload(super::connected()),
                        ("search", "loadMore") => self.search.load_more(super::connected()),
                        ("search", "fetchMore") => {
                            self.search.fetch_more(super::connected(), false)
                        }
                        ("details", "reload") => self.details.reload(super::connected()),
                        ("download", "start") => self.download.start(
                            &str_at(step, "sourceId"),
                            &str_at(step, "itemId"),
                            super::connected(),
                        ),
                        ("download", "remove") => self.download.remove(
                            &str_at(step, "sourceId"),
                            &str_at(step, "itemId"),
                            super::connected(),
                        ),
                        (other_q, other_m) => panic!("голден: нет метода {other_q}.{other_m}"),
                    };
                    self.run(&q, cmds);
                }
                "modelFetchMore" => self.model_fetch_more(),
                "settle" => self.settle(),
                "respond" | "respondError" => {
                    let token = i64_at(step, "token");
                    let frame = build_frame(step);
                    let owner = self
                        .token_owner
                        .get(&token)
                        .cloned()
                        .unwrap_or_else(|| panic!("голден: некому отдать ответ {token}"));
                    let cmds = match owner.as_str() {
                        "availability" => self.availability.apply(token, &frame),
                        "search" => self.search.apply(token, &frame),
                        "details" => self.details.apply(token, &frame),
                        "download" => self.download.apply(token, &frame),
                        other => panic!("голден: неизвестный владелец кадра {other}"),
                    };
                    self.run(&owner, cmds);
                }
                "connected" => super::set_connected(bool_at(step, "value")),
                other => panic!("голден: неизвестное действие {other}"),
            }
        }
    }

    /// Прогнать случай через ЧИСТЫЙ слой.
    pub fn replay_pure(case: &Value) -> Harness {
        super::reset_tokens();
        super::reset_localizer();
        super::set_connected(true);
        let mut harness = Harness::default();
        for step in case
            .get("steps")
            .and_then(Value::as_array)
            .expect("голден: у случая нет шагов")
        {
            harness.step(step);
        }
        // Генератор в конце случая ещё раз крутит цикл событий: незакрытый
        // дебаунс обязан сработать и там.
        harness.settle();
        harness
    }

    /// Строки модели в том же JSON-виде, что печатает голден.
    pub fn rows_json(rows: &[RemoteRow]) -> Value {
        Value::Array(rows.iter().map(RemoteRow::to_json).collect())
    }

    /// Ожидаемое итоговое состояние одного запроса из голдена.
    pub fn final_of<'a>(case: &'a Value, q: &str) -> &'a Value {
        case.get("final")
            .and_then(|f| f.get(q))
            .unwrap_or_else(|| panic!("голден: нет итогового состояния {q}"))
    }
}

// ============================================================================
// Стенд: те же случаи голдена на ЖИВЫХ QObject'ах
// ============================================================================

/// Самопроверка Qt-слоя без QML и без окна.
///
/// Чистый слой уже сверен с Python в `cargo test`; здесь проверяется ровно
/// то, чего чистый слой проверить не может:
///
/// * сигналы действительно долетают из C++ (а не «мы решили их послать»);
/// * дочерняя модель создаётся фабрикой, живёт ребёнком запроса и получает
///   те же команды;
/// * дебаунс — НАСТОЯЩИЙ QTimer: подряд идущие правки свойств склеиваются в
///   один запрос только потому, что так работает Qt, а не потому что так
///   написано в стенде;
/// * `dataChanged` дочерней модели несёт РОВНО одну роль (счётчик
///   «широких» — жёсткий критерий, как в партии 2);
/// * `sources` доезжает до QML как QVariantList вложенных QVariantMap.
///
/// Возврат: 0 — всё сошлось, 1 — есть расхождения. Молчаливого успеха по
/// таймауту нет и быть не может: стенд ничего не ждёт вслепую.
pub mod selftest {
    use super::golden::{self, Record, RecordKind};
    use super::qobject::{
        rexon_new_remote_availability_query, rexon_new_remote_details_query,
        rexon_new_remote_download_query, rexon_new_remote_search_query,
        RemoteAvailabilityQuery, RemoteDetailsQuery, RemoteDownloadQuery, RemoteSearchQuery,
    };
    use super::qobject::rexon_variant_to_json;
    use super::{Event, Request};
    use crate::remote_list_model::qobject::{
        rexon_delete_object, rexon_process_events, RemoteListModel,
    };
    use crate::remote_list_model::{ModelEvent, ROLE_BASE, ROLE_NAMES};
    use core::pin::Pin;
    use cxx_qt::QMetaObjectConnectionGuard;
    use cxx_qt_lib::{QByteArray, QModelIndex, QString, QVariant};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    pub const ARG: &str = "--discover-selftest";

    /// Журнал живых сигналов.
    type Journal = Arc<Mutex<Vec<Record>>>;

    /// Следы свойств, которые в мосте затирались (см. тесты чистого слоя).
    #[derive(Default)]
    struct Traces {
        error_text: Vec<String>,
        error: Vec<String>,
    }

    /// Живой набор объектов страницы: четыре запроса и модель выдачи.
    struct Page {
        availability: *mut RemoteAvailabilityQuery,
        search: *mut RemoteSearchQuery,
        details: *mut RemoteDetailsQuery,
        download: *mut RemoteDownloadQuery,
        model: *mut RemoteListModel,
        raw: Vec<*mut super::qobject::QObject>,
    }

    impl Page {
        fn new() -> Self {
            let availability = unsafe { rexon_new_remote_availability_query(core::ptr::null_mut()) };
            let search = unsafe { rexon_new_remote_search_query(core::ptr::null_mut()) };
            let details = unsafe { rexon_new_remote_details_query(core::ptr::null_mut()) };
            let download = unsafe { rexon_new_remote_download_query(core::ptr::null_mut()) };
            for raw in [availability, search, details, download] {
                assert!(!raw.is_null(), "фабрика вернула nullptr");
            }
            let search_query: *mut RemoteSearchQuery = search.cast();
            let model = unsafe { (*search_query).model() }.cast::<RemoteListModel>();
            assert!(!model.is_null(), "запрос поиска не создал модель");
            Page {
                availability: availability.cast(),
                search: search_query,
                details: details.cast(),
                download: download.cast(),
                model,
                // Модель — ребёнок запроса, отдельно её удалять нельзя.
                raw: vec![availability, search, details, download],
            }
        }

        // Изменяемая ссылка из неизменяемой: объектами владеет Qt, а не
        // Rust. Та же схема, что у стенда партии 2.
        #[allow(clippy::mut_from_ref)]
        fn availability(&self) -> Pin<&mut RemoteAvailabilityQuery> {
            unsafe { Pin::new_unchecked(&mut *self.availability) }
        }

        #[allow(clippy::mut_from_ref)]
        fn search(&self) -> Pin<&mut RemoteSearchQuery> {
            unsafe { Pin::new_unchecked(&mut *self.search) }
        }

        #[allow(clippy::mut_from_ref)]
        fn details(&self) -> Pin<&mut RemoteDetailsQuery> {
            unsafe { Pin::new_unchecked(&mut *self.details) }
        }

        #[allow(clippy::mut_from_ref)]
        fn download(&self) -> Pin<&mut RemoteDownloadQuery> {
            unsafe { Pin::new_unchecked(&mut *self.download) }
        }

        #[allow(clippy::mut_from_ref)]
        fn model(&self) -> Pin<&mut RemoteListModel> {
            unsafe { Pin::new_unchecked(&mut *self.model) }
        }

        fn destroy(self) {
            for raw in self.raw {
                unsafe { rexon_delete_object(raw) };
            }
        }
    }

    fn push(journal: &Journal, q: &str, event: Event) {
        journal.lock().expect("журнал").push(Record {
            q: q.to_string(),
            kind: RecordKind::Query(event),
        });
    }

    /// Подписка на сигналы базы Query. Макрос, потому что типы разные, а
    /// имена методов одинаковые — ровно та же причина, что у
    /// `impl_query_base!`.
    macro_rules! connect_base {
        ($guards:expr, $journal:expr, $traces:expr, $object:expr, $ty:ty, $name:literal) => {{
            let raw = unsafe { $object.get_unchecked_mut() as *mut $ty };
            let pin = || unsafe { Pin::new_unchecked(&mut *raw) };

            let sink = Arc::clone($journal);
            $guards.push(pin().on_status_changed(move |_, status| {
                push(&sink, $name, Event::Status(status));
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_querying_changed(move |_| {
                push(&sink, $name, Event::Querying);
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_error_changed(move |_| {
                push(&sink, $name, Event::ErrorChanged);
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_forward_error_changed(move |_| {
                push(&sink, $name, Event::ForwardErrorChanged);
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_finished(move |_| {
                push(&sink, $name, Event::Finished);
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_error_occurred(move |_, text| {
                push(&sink, $name, Event::ErrorOccurred(text.to_string()));
            }));
            let sink = Arc::clone($journal);
            $guards.push(pin().on_app_error_occurred(move |_, text| {
                push(&sink, $name, Event::AppError(text.to_string()));
            }));
            // Кадр разбирается ОБРАТНО из байтов: стенд обязан проверять то,
            // что реально уехало, а не намерение это отправить.
            let sink = Arc::clone($journal);
            $guards.push(pin().on_send_request(move |_, token, field, request| {
                let bytes = request.as_slice().to_vec();
                let request = Request::decode(&bytes)
                    .unwrap_or_else(|| panic!("стенд: кадр {} не разобрался", field));
                push(
                    &sink,
                    $name,
                    Event::Sent {
                        token,
                        field: request.field(),
                        request,
                    },
                );
            }));
            let _ = &$traces;
        }};
    }

    /// Счётчики стенда.
    struct Report {
        failures: Vec<String>,
        wide_updates: usize,
        cases: usize,
        events: usize,
        /// Сколько описаний источников прошло через сверку за весь прогон.
        /// Ноль — значит сравнивать было нечего, и зелёный цвет ничего не
        /// значит: у большинства случаев список источников пуст законно.
        sources_seen: usize,
    }

    impl Report {
        fn check(&mut self, condition: bool, message: impl FnOnce() -> String) {
            if !condition {
                self.failures.push(message());
            }
        }
    }

    /// Подписка на сигналы дочерней модели выдачи.
    ///
    /// Структурных сигналов (modelReset, rowsInserted) здесь нет и быть не
    /// может: они приватные (QPrivateSignal), из Rust на них не подписаться.
    /// Их порядок проверяет чистый слой в `cargo test`.
    fn connect_model(
        model: Pin<&mut RemoteListModel>,
        journal: &Journal,
        wide: &Arc<Mutex<usize>>,
    ) -> Vec<QMetaObjectConnectionGuard> {
        let mut guards = Vec::new();
        let raw = unsafe { model.get_unchecked_mut() as *mut RemoteListModel };
        let pin = || unsafe { Pin::new_unchecked(&mut *raw) };

        fn push_model(journal: &Journal, event: ModelEvent) {
            journal.lock().expect("журнал").push(Record {
                q: "search.model".to_string(),
                kind: RecordKind::Model(event),
            });
        }

        let sink = Arc::clone(journal);
        guards.push(pin().on_count_changed(move |_| {
            push_model(&sink, ModelEvent::CountChanged);
        }));
        let sink = Arc::clone(journal);
        guards.push(pin().on_has_more_changed(move |_, value| {
            push_model(&sink, ModelEvent::HasMoreChanged(value));
        }));
        let sink = Arc::clone(journal);
        guards.push(pin().on_req_fetch_more(move |_, row| {
            push_model(&sink, ModelEvent::ReqFetchMore(row as usize));
        }));
        let sink = Arc::clone(journal);
        let wide = Arc::clone(wide);
        guards.push(pin().on_data_changed(move |_, top_left, bottom_right, roles| {
            let names: Vec<&'static str> = (0..roles.len())
                .filter_map(|i| roles.get(i).copied())
                .filter_map(|role| {
                    usize::try_from(role - ROLE_BASE)
                        .ok()
                        .and_then(|slot| ROLE_NAMES.get(slot).copied())
                })
                .collect();
            let role = if names.len() == 1 {
                names[0]
            } else {
                *wide.lock().expect("счётчик широких") += 1;
                "<широкий>"
            };
            push_model(
                &sink,
                ModelEvent::DataChanged {
                    first: top_left.row() as usize,
                    last: bottom_right.row() as usize,
                    role,
                },
            );
        }));
        guards
    }

    /// Значение QVariant в JSON-виде голдена.
    fn variant_text(value: &QVariant) -> String {
        if let Some(text) = value.value::<QString>() {
            return text.to_string();
        }
        match value.value::<i32>() {
            Some(number) => number.to_string(),
            None => String::new(),
        }
    }

    /// Строки живой модели в том же JSON-виде, что печатает голден.
    fn rows_json(model: &RemoteListModel) -> Value {
        let mut rows = Vec::new();
        for row in 0..model.count() {
            let mut object = serde_json::Map::new();
            for (key, value) in model.get(row).iter() {
                let name = key.to_string();
                let json = if name == "acquisitionState" {
                    Value::from(value.value::<i32>().unwrap_or_default())
                } else {
                    Value::String(variant_text(value))
                };
                object.insert(name, json);
            }
            rows.push(Value::Object(object));
        }
        Value::Array(rows)
    }

    fn strings_of(list: &cxx_qt_lib::QStringList) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Прогон одного случая голдена на живых объектах.
    fn run_case(case: &Value, report: &mut Report) {
        super::reset_tokens();
        super::reset_localizer();
        super::set_connected(true);

        let page = Page::new();
        let journal: Journal = Arc::new(Mutex::new(Vec::new()));
        let wide = Arc::new(Mutex::new(0usize));
        let traces = Arc::new(Mutex::new(Traces::default()));
        let mut guards: Vec<QMetaObjectConnectionGuard> = Vec::new();

        connect_base!(&mut guards, &journal, traces, page.availability(), RemoteAvailabilityQuery, "availability");
        connect_base!(&mut guards, &journal, traces, page.search(), RemoteSearchQuery, "search");
        connect_base!(&mut guards, &journal, traces, page.details(), RemoteDetailsQuery, "details");
        connect_base!(&mut guards, &journal, traces, page.download(), RemoteDownloadQuery, "download");

        // Собственные сигналы запросов.
        let sink = Arc::clone(&journal);
        guards.push(page.availability().on_sources_changed(move |_| {
            push(&sink, "availability", Event::Prop("sourcesChanged"));
        }));

        for (signal, name) in [
            ("sourceId", "sourceIdChanged"),
            ("query", "queryChanged"),
            ("sortKey", "sortKeyChanged"),
            ("tags", "tagsChanged"),
            ("browsingEnabled", "browsingEnabledChanged"),
        ] {
            let sink = Arc::clone(&journal);
            let guard = match signal {
                "sourceId" => page.search().on_source_id_changed(move |_| {
                    push(&sink, "search", Event::Prop("sourceIdChanged"));
                }),
                "query" => page.search().on_query_changed(move |_| {
                    push(&sink, "search", Event::Prop("queryChanged"));
                }),
                "sortKey" => page.search().on_sort_key_changed(move |_| {
                    push(&sink, "search", Event::Prop("sortKeyChanged"));
                }),
                "tags" => page.search().on_tags_changed(move |_| {
                    push(&sink, "search", Event::Prop("tagsChanged"));
                }),
                _ => page.search().on_browsing_enabled_changed(move |_| {
                    push(&sink, "search", Event::Prop("browsingEnabledChanged"));
                }),
            };
            let _ = name;
            guards.push(guard);
        }

        // stateChanged и errorChanged поиска ещё и снимают след свойств,
        // которые в мосте затирались (см. тесты чистого слоя).
        let sink = Arc::clone(&journal);
        let trace = Arc::clone(&traces);
        guards.push(page.search().on_state_changed(move |query| {
            trace
                .lock()
                .expect("следы")
                .error_text
                .push(query.error_text().to_string());
            push(&sink, "search", Event::Prop("stateChanged"));
        }));
        let trace = Arc::clone(&traces);
        guards.push(page.search().on_error_changed(move |query| {
            trace
                .lock()
                .expect("следы")
                .error
                .push(query.error().to_string());
        }));

        let sink = Arc::clone(&journal);
        guards.push(page.details().on_source_id_changed(move |_| {
            push(&sink, "details", Event::Prop("sourceIdChanged"));
        }));
        let sink = Arc::clone(&journal);
        guards.push(page.details().on_item_id_changed(move |_| {
            push(&sink, "details", Event::Prop("itemIdChanged"));
        }));
        let sink = Arc::clone(&journal);
        guards.push(page.details().on_loaded(move |_| {
            push(&sink, "details", Event::Prop("loaded"));
        }));

        let sink = Arc::clone(&journal);
        guards.push(page.download().on_accepted(move |_, source_id, id| {
            push(
                &sink,
                "download",
                Event::Accepted {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                },
            );
        }));
        let sink = Arc::clone(&journal);
        guards.push(page.download().on_rejected(move |_, source_id, id, error| {
            push(
                &sink,
                "download",
                Event::Rejected {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                    error: error.to_string(),
                },
            );
        }));
        let sink = Arc::clone(&journal);
        guards.push(page.download().on_removed(move |_, source_id, id| {
            push(
                &sink,
                "download",
                Event::Removed {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                },
            );
        }));
        let sink = Arc::clone(&journal);
        guards.push(page.download().on_remove_failed(move |_, source_id, id, error| {
            push(
                &sink,
                "download",
                Event::RemoveFailed {
                    source_id: source_id.to_string(),
                    id: id.to_string(),
                    error: error.to_string(),
                },
            );
        }));

        let model_guards = connect_model(page.model(), &journal, &wide);

        // ── шаги сценария ────────────────────────────────────────────────
        let name = golden::str_at(case, "name");
        for step in case
            .get("steps")
            .and_then(Value::as_array)
            .expect("голден: у случая нет шагов")
        {
            match golden::str_at(step, "op").as_str() {
                "set" => {
                    let q = golden::str_at(step, "q");
                    let prop = golden::str_at(step, "name");
                    let value = step.get("value").cloned().unwrap_or(Value::Null);
                    match (q.as_str(), prop.as_str()) {
                        ("search", "sourceId") => page
                            .search()
                            .set_source_id(QString::from(value.as_str().unwrap_or_default())),
                        ("search", "query") => page
                            .search()
                            .set_query(QString::from(value.as_str().unwrap_or_default())),
                        ("search", "sortKey") => page
                            .search()
                            .set_sort_key(QString::from(value.as_str().unwrap_or_default())),
                        ("search", "tags") => {
                            let tags: Vec<String> = value
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .map(|v| v.as_str().unwrap_or_default().to_string())
                                        .collect()
                                })
                                .unwrap_or_default();
                            page.search().set_tags(super::qstringlist(&tags));
                        }
                        ("search", "browsingEnabled") => page
                            .search()
                            .set_browsing_enabled(value.as_bool().unwrap_or(false)),
                        ("details", "sourceId") => page
                            .details()
                            .set_source_id(QString::from(value.as_str().unwrap_or_default())),
                        ("details", "itemId") => page
                            .details()
                            .set_item_id(QString::from(value.as_str().unwrap_or_default())),
                        ("availability", "forwardError") => page
                            .availability()
                            .set_forward_error(value.as_bool().unwrap_or(false)),
                        (other_q, other_p) => {
                            report.check(false, || format!("{name}: нет свойства {other_q}.{other_p}"))
                        }
                    }
                }
                "call" => {
                    let q = golden::str_at(step, "q");
                    let method = golden::str_at(step, "method");
                    match (q.as_str(), method.as_str()) {
                        ("availability", "reload") => page.availability().reload(),
                        ("search", "reload") => page.search().reload(),
                        ("search", "loadMore") => page.search().load_more(),
                        ("search", "fetchMore") => page.search().fetch_more(0),
                        ("details", "reload") => page.details().reload(),
                        ("download", "start") => page.download().start(
                            &QString::from(&golden::str_at(step, "sourceId")),
                            &QString::from(&golden::str_at(step, "itemId")),
                        ),
                        ("download", "remove") => page.download().remove(
                            &QString::from(&golden::str_at(step, "sourceId")),
                            &QString::from(&golden::str_at(step, "itemId")),
                        ),
                        (other_q, other_m) => {
                            report.check(false, || format!("{name}: нет метода {other_q}.{other_m}"))
                        }
                    }
                    // Склад состояний приобретения доставляет очередью цикла
                    // событий (в Python это была прямая связь) — прокручиваем,
                    // чтобы dataChanged успел прийти.
                    rexon_process_events(30);
                }
                "modelFetchMore" => page.model().fetch_more(&QModelIndex::default()),
                // Дебаунс — НАСТОЯЩИЙ таймер: крутим цикл событий дольше его
                // интервала. Если бы правки не склеивались, здесь пришло бы
                // несколько кадров, и сверка журнала это поймала бы.
                "settle" => rexon_process_events(golden::i64_at(step, "ms").max(300) as i32),
                "respond" | "respondError" => {
                    let token = golden::i64_at(step, "token");
                    let frame = QByteArray::from(&golden::build_frame(step)[..]);
                    // Кому отдать ответ — известно из уже записанного кадра.
                    let owner = journal
                        .lock()
                        .expect("журнал")
                        .iter()
                        .find_map(|record| match &record.kind {
                            RecordKind::Query(Event::Sent { token: t, .. }) if *t == token => {
                                Some(record.q.clone())
                            }
                            _ => None,
                        });
                    match owner.as_deref() {
                        Some("availability") => {
                            page.availability().deliver_response(token, &frame)
                        }
                        Some("search") => page.search().deliver_response(token, &frame),
                        Some("details") => page.details().deliver_response(token, &frame),
                        Some("download") => page.download().deliver_response(token, &frame),
                        _ => report.check(false, || {
                            format!("{name}: некому отдать ответ {token}")
                        }),
                    }
                    rexon_process_events(30);
                }
                "connected" => super::set_connected(golden::bool_at(step, "value")),
                other => report.check(false, || format!("{name}: неизвестное действие {other}")),
            }
        }
        // Незакрытый дебаунс обязан сработать и в конце случая.
        rexon_process_events(300);

        // ── сверка журналов ──────────────────────────────────────────────
        //
        // Порядок сверяется ПО КАЖДОМУ объекту отдельно, а не одной общей
        // лентой, и это осознанная поблажка ровно в одном месте: общий склад
        // состояний приобретения доставляет обновление очередью цикла
        // событий (`CxxQtThread::queue`), а в Python это прямая связь. Значит
        // `dataChanged` карточки приходит ПОСЛЕ синхронного хвоста
        // скачивания, а не внутри него. Внутри каждого объекта порядок
        // сверяется посимвольно, и отдельно сверяется, что общий состав
        // событий совпал.
        let actual = journal.lock().expect("журнал").clone();
        report.events += actual.len();
        let expected: Vec<Record> = golden::expected_records(case)
            .into_iter()
            .filter(|record| match &record.kind {
                // Структурные сигналы модели приватные — из Rust на них не
                // подписаться (см. connect_model).
                RecordKind::Model(event) => !event.is_structural(),
                RecordKind::Query(_) => true,
            })
            .collect();

        let mut owners: Vec<String> = expected.iter().map(|r| r.q.clone()).collect();
        owners.extend(actual.iter().map(|r| r.q.clone()));
        owners.sort();
        owners.dedup();
        for owner in owners {
            let want: Vec<&Record> = expected.iter().filter(|r| r.q == owner).collect();
            let got: Vec<&Record> = actual.iter().filter(|r| r.q == owner).collect();
            report.check(want == got, || {
                format!("{name} · {owner}: порядок сигналов разошёлся\n  получено: {got:?}\n  ожидалось: {want:?}")
            });
        }
        report.check(actual.len() == expected.len(), || {
            format!(
                "{name}: сигналов {} вместо {}",
                actual.len(),
                expected.len()
            )
        });

        // ── сверка свойств у ЖИВЫХ объектов ──────────────────────────────
        let want = golden::final_of(case, "availability");
        let availability = page.availability();
        report.check(availability.status() == golden::i64_at(want, "status") as i32, || {
            format!("{name}: статус доступности {}", availability.status())
        });
        report.check(availability.error().to_string() == golden::str_at(want, "error"), || {
            format!("{name}: ошибка доступности «{}»", availability.error())
        });
        report.check(
            availability.forward_error() == golden::bool_at(want, "forwardError"),
            || format!("{name}: forwardError доступности"),
        );
        report.check(
            availability.default_source_id().to_string()
                == golden::str_at(want, "defaultSourceId"),
            || format!("{name}: источник по умолчанию"),
        );
        // Списком источников проверяется ровно то, что увидит QML:
        // QVariantList вложенных QVariantMap, а не Rust-структура. Перевод в
        // JSON делает сам Qt (QJsonDocument::fromVariant) — так проверяется
        // настоящий тип каждого значения, а не то, во что его согласился бы
        // сконвертировать QVariant::canConvert.
        let sources: Value = serde_json::from_slice(
            rexon_variant_to_json(&QVariant::from(&availability.sources())).as_slice(),
        )
        .unwrap_or(Value::Null);
        // ⚠️ Сверка в два приёма — ровно та же, что у чистого слоя в
        // `cargo test`, и по той же причине. Эталон снят с ЖИВОГО
        // Python-моста, а тот знает протокол waywallen до 0.3.7: карт
        // `*Text` в нём нет вовсе, а варианты фильтра лежат плоским списком
        // подписей (`values`), потому что до 0.3.8 подпись И БЫЛА значением.
        // С 0.3.8 источник присылает тройку `{value, label, labelText}`:
        // демону уходит `value` («CG»), человеку показывается `label`
        // («Комп. графика»). Перевод подписи — правильное поведение, отбор
        // от него не страдает, и красное здесь означало лишь одно: стенд
        // сравнивал ответ 0.3.8 с эталоном Python-эпохи БУКВАЛЬНО, тогда как
        // чистый слой давно складывает их обратно.
        //
        //   1. всё, что отдавал Python, доезжает без изменений;
        //   2. новые карты подписей и машинные ключи на месте — ниже.
        let folded = golden::strip_message_maps(&sources);
        report.check(folded == want["sources"], || {
            format!("{name}: описания источников разошлись\n  получено: {folded}")
        });
        for problem in golden::message_map_problems(&sources) {
            report.check(false, || format!("{name}: {problem}"));
        }
        // ⭐ Доказательство, что сверке было что сравнивать: случай с пустым
        // списком источников проходит её вхолостую.
        report.sources_seen += golden::source_count(&sources);

        let want = golden::final_of(case, "search");
        let search = page.search();
        report.check(search.status() == golden::i64_at(want, "status") as i32, || {
            format!("{name}: статус поиска {}", search.status())
        });
        report.check(search.source_id().to_string() == golden::str_at(want, "sourceId"), || {
            format!("{name}: источник поиска")
        });
        report.check(search.query().to_string() == golden::str_at(want, "query"), || {
            format!("{name}: строка поиска")
        });
        report.check(search.sort_key().to_string() == golden::str_at(want, "sortKey"), || {
            format!("{name}: сортировка")
        });
        report.check(strings_of(&search.tags()) == golden::strings_at(want, "tags"), || {
            format!("{name}: фильтры")
        });
        report.check(
            search.browsing_enabled() == golden::bool_at(want, "browsingEnabled"),
            || format!("{name}: разрешение просмотра"),
        );
        report.check(search.has_more() == golden::bool_at(want, "hasMore"), || {
            format!("{name}: догрузка")
        });
        {
            let traces = traces.lock().expect("следы");
            report.check(
                traces.error_text == golden::strings_at(want, "errorTextTrace"),
                || {
                    format!(
                        "{name}: след текста ошибки источника {:?}",
                        traces.error_text
                    )
                },
            );
            report.check(traces.error == golden::strings_at(want, "errorTrace"), || {
                format!("{name}: след ошибки поиска {:?}", traces.error)
            });
        }
        let rows = rows_json(unsafe { &*page.model });
        report.check(rows == want["rows"], || {
            format!("{name}: строки выдачи разошлись\n  получено: {rows}")
        });

        let want = golden::final_of(case, "details");
        let details = page.details();
        report.check(details.status() == golden::i64_at(want, "status") as i32, || {
            format!("{name}: статус деталей")
        });
        report.check(details.error().to_string() == golden::str_at(want, "error"), || {
            format!("{name}: ошибка деталей")
        });
        report.check(details.source_id().to_string() == golden::str_at(want, "sourceId"), || {
            format!("{name}: источник деталей")
        });
        report.check(details.item_id().to_string() == golden::str_at(want, "itemId"), || {
            format!("{name}: предмет деталей")
        });
        report.check(
            details.description().to_string() == golden::str_at(want, "description"),
            || format!("{name}: описание"),
        );
        report.check(details.size().to_string() == golden::str_at(want, "size"), || {
            format!("{name}: размер")
        });
        report.check(details.width() == golden::i64_at(want, "width") as i32, || {
            format!("{name}: ширина")
        });
        report.check(details.height() == golden::i64_at(want, "height") as i32, || {
            format!("{name}: высота")
        });
        report.check(strings_of(&details.tags()) == golden::strings_at(want, "tags"), || {
            format!("{name}: теги деталей")
        });
        report.check(details.web_url().to_string() == golden::str_at(want, "webUrl"), || {
            format!("{name}: ссылка на страницу")
        });

        let want = golden::final_of(case, "download");
        let download = page.download();
        report.check(download.status() == golden::i64_at(want, "status") as i32, || {
            format!("{name}: статус скачивания")
        });
        report.check(download.error().to_string() == golden::str_at(want, "error"), || {
            format!("{name}: ошибка скачивания")
        });

        report.wide_updates += *wide.lock().expect("счётчик широких");

        drop(model_guards);
        drop(guards);
        page.destroy();
    }

    /// Точка входа стенда. Код возврата — приговор.
    pub fn run() -> i32 {
        // Стенду хватает ядра приложения: ни окон, ни QML здесь нет.
        let mut app = cxx_qt_lib::QCoreApplication::new();
        if let Some(app) = app.as_mut() {
            app.set_application_name(&QString::from("Rexon (стенд запросов Discover)"));
        }
        // Эталон — на исходном языке (см. `golden_parity_with_python_queries`).
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));

        let doc = golden::load();
        let cases = doc["cases"].as_array().expect("голден: cases").clone();
        let mut report = Report {
            failures: Vec::new(),
            wide_updates: 0,
            cases: cases.len(),
            events: 0,
            sources_seen: 0,
        };

        // Значение по умолчанию страницы — то же, что проверяет чистый слой,
        // но здесь оно считается тем же кодом, что и в живом запросе.
        let defaults = doc["pickDefault"].as_array().expect("голден: pickDefault");
        // Тот же класс, что с пустым списком случаев ниже: таблица без строк
        // прогнала бы перебор ни разу и промолчала.
        report.check(!defaults.is_empty(), || {
            "в эталоне нет ни одной строки pickDefault — сверять нечего".to_string()
        });
        for case in defaults {
            let sources: Vec<Value> = golden::strings_at(case, "sources")
                .iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect();
            let daemon = golden::str_at(case, "daemonDefault");
            let expected = golden::str_at(case, "expected");
            let actual = super::pick_default(&sources, &daemon);
            report.check(actual == expected, || {
                format!("значение по умолчанию: «{actual}» вместо «{expected}»")
            });
        }

        // ⭐ Пустая выборка = провал, а не успех: голден без случаев прогнал бы
        // перебор НИ РАЗУ и объявил стенд зелёным, не проверив ничего. Тот же
        // класс, на котором попался QML-стенд: сценарий Мастерской шёл на пустой
        // модели и сверял ноль с нулём.
        report.check(!cases.is_empty(), || {
            "в эталоне нет ни одного случая — стенду нечего проверять".to_string()
        });
        for case in &cases {
            run_case(case, &mut report);
        }

        // ⭐ Тот же класс, что и пустой эталон, только изнутри: сверка описаний
        // источников законно проходит вхолостую там, где демон их не прислал
        // (ошибка, отключённый сокет). Если ВЕСЬ прогон оказался таким, она не
        // доказала ничего — и молчать об этом нельзя.
        report.check(report.sources_seen > 0, || {
            "ни один случай не принёс описаний источников — сверять было нечего".to_string()
        });
        // И сигналов: журнал обязан быть содержательным, иначе сверка порядка
        // тоже сравнивала пустоту с пустотой. Порог тот же, что у чистого слоя.
        let events = report.events;
        report.check(events > 100, || {
            format!("сигналов за прогон всего {events} — эталон подозрительно пуст")
        });

        println!(
            "REXON-DISCOVER-SELFTEST cases={} events={} sources={} wideUpdates={} failures={}",
            report.cases,
            report.events,
            report.sources_seen,
            report.wide_updates,
            report.failures.len()
        );
        for failure in &report.failures {
            println!("REXON-DISCOVER-SELFTEST-FAIL {failure}");
        }
        // «Широких» dataChanged быть не должно ни одного: точечное
        // обновление роли не имеет права вырождаться в «изменились все».
        let ok = report.failures.is_empty() && report.wide_updates == 0;
        println!(
            "REXON-DISCOVER-SELFTEST-VERDICT {} failures={} wideUpdates={}",
            if ok { "OK" } else { "FAIL" },
            report.failures.len(),
            report.wide_updates
        );
        i32::from(!ok)
    }
}

#[cfg(test)]
mod tests {
    use super::golden::{self, RecordKind};
    use super::*;

    /// Автор из ответа деталей: вендорная панель берёт его запасным, когда у
    /// строки выдачи автора нет (`details?.author`, RemoteDetailPanel.qml и
    /// RemoteInfoPage.qml). Свойство есть в `module.qmltypes` апстрима; его
    /// отсутствие у нас нашла сверка `tests/waywallen_qmltypes_contract.rs`.
    #[test]
    fn details_carry_author_and_reload_clears_it() {
        let mut state = DetailsState::default();
        state.set_item_id("42");
        let token = state
            .reload(true)
            .iter()
            .find_map(|c| match c {
                Cmd::Signal(Event::Sent { token, .. }) => Some(*token),
                _ => None,
            })
            .expect("запрос деталей ушёл");
        let frame = golden::build_frame(&serde_json::json!({
            "payload": { "kind": "remote_details", "author": "hypengw" }
        }));
        state.apply(token, &frame);
        assert_eq!(state.author, "hypengw");
        state.reload(true);
        assert_eq!(state.author, "", "прошлый автор не держится при новом запросе");
    }

    /// waywallen 0.5.0: аватар автора в деталях (`RemoteDetailsResponse.
    /// author_avatar_url`) — заполняется ответом и сбрасывается новым
    /// запросом, как остальные поля панели.
    #[test]
    fn details_carry_author_avatar_and_reload_clears_it() {
        let mut state = DetailsState::default();
        state.set_item_id("42");
        let token = state
            .reload(true)
            .iter()
            .find_map(|c| match c {
                Cmd::Signal(Event::Sent { token, .. }) => Some(*token),
                _ => None,
            })
            .expect("запрос деталей ушёл");
        let frame = golden::build_frame(&serde_json::json!({
            "payload": {
                "kind": "remote_details",
                "authorAvatarUrl": "https://example.invalid/hypengw.png"
            }
        }));
        let cmds = state.apply(token, &frame);
        assert_eq!(state.author_avatar_url, "https://example.invalid/hypengw.png");
        assert!(
            cmds.iter().any(|c| matches!(c, Cmd::Signal(Event::Prop("loaded")))),
            "свойство извещается сигналом loaded"
        );
        state.reload(true);
        assert_eq!(state.author_avatar_url, "", "прошлый аватар не держится при новом запросе");
    }

    /// waywallen 0.5.0: строка выдачи несёт аватар автора из `RemoteItem`
    /// (роль `authorAvatarUrl`, `remote_model.cppm`).
    #[test]
    fn search_row_carries_author_avatar_url() {
        let row = row_from_item(&pb::RemoteItem {
            id: "a1".into(),
            author_avatar_url: "https://example.invalid/a1.png".into(),
            ..Default::default()
        });
        assert_eq!(row.author_avatar_url, "https://example.invalid/a1.png");
        assert_eq!(row.to_json()["authorAvatarUrl"], "https://example.invalid/a1.png");
    }

    /// Имя и подсказка источника без сообщения плагина доходят до страницы
    /// запасным текстом, как у вендора, а не пустой картой: иначе в списке
    /// источников стоит техническое `wallhaven` вместо `Wallhaven`.
    #[test]
    fn source_name_and_hint_fall_back_to_plain_text() {
        let shown = |message: &Value| {
            rexon_core::plugin_l10n::resolve(
                &Default::default(),
                message["pluginId"].as_str().unwrap(),
                message["msgid"].as_str().unwrap(),
                "",
                "en",
            )
        };
        let plain = source_info(&pb::RemoteSourceInfo {
            id: "wallhaven".into(),
            name: "wallhaven".into(),
            display_name: "Wallhaven".into(),
            remote_hint: "Sign in to see more".into(),
            ..Default::default()
        });
        assert_eq!(shown(&plain["displayNameText"]), "Wallhaven");
        assert_eq!(shown(&plain["remoteHintText"]), "Sign in to see more");
        assert_eq!(plain["displayNameText"]["pluginId"], "");
        assert!(golden::message_map_problems(&Value::Array(vec![plain])).is_empty());

        // Сообщение плагина есть — оно и уходит, запасной текст не нужен.
        let message = pb::PluginMessage {
            plugin_id: "org.example".into(),
            msgid: "Example".into(),
            ..Default::default()
        };
        let translated = source_info(&pb::RemoteSourceInfo {
            display_name: "Plain".into(),
            display_name_text: Some(message),
            ..Default::default()
        });
        assert_eq!(translated["displayNameText"]["pluginId"], "org.example");
        assert_eq!(translated["displayNameText"]["msgid"], "Example");

        // Ни сообщения, ни запасного текста — пустая карта: имя выберет QML.
        let bare = source_info(&pb::RemoteSourceInfo::default());
        assert_eq!(bare["displayNameText"]["msgid"], "");
        assert_eq!(bare["remoteHintText"]["msgid"], "");
    }

    /// Голден снят с ЖИВЫХ Python-запросов
    /// (tests/golden/discover_queries_cases.py). Здесь он прогоняется через
    /// ЧИСТЫЙ слой порта: какие кадры уходят демону, в каком порядке летят
    /// сигналы (свои, базы, дочерней модели, общего склада) и какими
    /// оказываются свойства. Qt-слой той же сверкой проверяет стенд
    /// `selftest::run()`.
    ///
    /// Один тест на всё, а не пять: чистый слой пользуется общими на мост
    /// счётчиком токенов и переводчиком (в Python это те же модульные
    /// переменные), и два теста, идущие параллельно, толкались бы за них.
    #[test]
    fn golden_parity_with_python_queries() {
        let _tokens = super::token_test_lock();
        // Язык — исходный: тексты ошибок и подписи источников с 30.09 —
        // msgid каталога (русский — переводом), а язык процесса общий и
        // соседи крейта ставят именно исходный. Эталон хранит английские
        // msgid там, где Python-эпоха писала русский.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let doc = golden::load();

        // Значение по умолчанию — статический метод, у него нет ни кадров,
        // ни сигналов, поэтому голден снимает его отдельной таблицей.
        let defaults = doc["pickDefault"].as_array().expect("голден: pickDefault");
        // Пустая таблица прогнала бы перебор ни разу и позеленела впустую.
        assert!(!defaults.is_empty(), "в эталоне нет ни одной строки pickDefault");
        for case in defaults {
            let sources: Vec<Value> = golden::strings_at(case, "sources")
                .iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect();
            let daemon = golden::str_at(case, "daemonDefault");
            assert_eq!(
                pick_default(&sources, &daemon),
                golden::str_at(case, "expected"),
                "значение по умолчанию для {sources:?} при умолчании демона «{daemon}»"
            );
        }

        let cases = doc["cases"].as_array().expect("голден: cases");
        assert!(!cases.is_empty(), "голден пуст");
        let mut records = 0usize;
        let mut sources_seen = 0usize;
        for case in cases {
            let name = golden::str_at(case, "name");
            let harness = golden::replay_pure(case);
            let expected = golden::expected_records(case);
            assert_eq!(harness.journal, expected, "{name}: последовательность сигналов");
            records += harness.journal.len();
            sources_seen += check_final_json(&name, &harness, case);
        }
        // Страховка от «сравнили пустоту с пустотой»: журналы обязаны быть
        // содержательными (пустой журнал допустим только у случая, который
        // проверяет ИМЕННО молчание).
        assert!(records > 100, "проиграно всего {records} сигналов — эталон подозрительно пуст");
        // Та же страховка для описаний источников: у большинства случаев их
        // законно нет вовсе, и если бы их не оказалось НИ У ОДНОГО, сверка
        // описаний прошла бы весь прогон вхолостую.
        assert!(
            sources_seen > 0,
            "ни один случай не принёс описаний источников — сверять было нечего"
        );
    }

    /// Возвращает, сколько описаний источников прошло сверку: вызывающий
    /// складывает их за прогон и краснеет, если не набралось ни одного.
    fn check_final_json(name: &str, harness: &golden::Harness, case: &Value) -> usize {
        let want = golden::final_of(case, "availability");
        assert_eq!(
            harness.availability.base.status as i64,
            golden::i64_at(want, "status"),
            "{name}: статус доступности"
        );
        assert_eq!(
            harness.availability.base.error,
            golden::str_at(want, "error"),
            "{name}: ошибка доступности"
        );
        assert_eq!(
            harness.availability.base.forward_error,
            golden::bool_at(want, "forwardError"),
            "{name}: forwardError доступности"
        );
        // ⚠️ Эталон снят с ЖИВОГО Python-моста, а тот про waywallen 0.3.7
        // не знает: карт `*Text` (локализуемые подписи плагинов) в нём нет
        // вовсе. Поэтому сверка идёт в два приёма — иначе пришлось бы либо
        // дописать эталон руками (и он перестал бы быть снимком), либо
        // выкинуть новые поля из моста.
        //
        //   1. всё, что отдавал Python, доезжает без изменений;
        //   2. карты сообщений на месте — проверка ниже.
        let sources = Value::Array(harness.availability.sources.clone());
        assert_eq!(
            golden::strip_message_maps(&sources),
            want["sources"],
            "{name}: описания источников"
        );
        assert_eq!(
            golden::message_map_problems(&sources),
            Vec::<String>::new(),
            "{name}: карты подписей источников"
        );
        assert_eq!(
            harness.availability.default_source_id,
            golden::str_at(want, "defaultSourceId"),
            "{name}: источник по умолчанию"
        );

        let want = golden::final_of(case, "search");
        assert_eq!(
            harness.search.base.status as i64,
            golden::i64_at(want, "status"),
            "{name}: статус поиска"
        );
        // Та же история, что с errorText ниже, только с другой стороны: у
        // поиска `error` базы затирается текстом ошибки ИСТОЧНИКА, поэтому
        // сверяется след в моменты errorChanged, а не конечное значение.
        assert_eq!(
            harness.error_trace,
            golden::strings_at(want, "errorTrace"),
            "{name}: след ошибки поиска"
        );
        assert_eq!(
            harness.search.source_id,
            golden::str_at(want, "sourceId"),
            "{name}: источник поиска"
        );
        assert_eq!(
            harness.search.query,
            golden::str_at(want, "query"),
            "{name}: строка поиска"
        );
        assert_eq!(
            harness.search.sort_key,
            golden::str_at(want, "sortKey"),
            "{name}: сортировка"
        );
        assert_eq!(
            harness.search.tags,
            golden::strings_at(want, "tags"),
            "{name}: фильтры"
        );
        assert_eq!(
            harness.search.browsing_enabled,
            golden::bool_at(want, "browsingEnabled"),
            "{name}: разрешение просмотра"
        );
        assert_eq!(
            harness.rows.has_more(),
            golden::bool_at(want, "hasMore"),
            "{name}: догрузка"
        );
        // ⚠️ Починка при порте (правило #16), а не расхождение по недосмотру.
        //
        // В мосте `errorText` и `error` базы — ОДИН И ТОТ ЖЕ атрибут
        // `self._error`: `_apply_page` пишет туда текст ошибки ИСТОЧНИКА, а
        // `_dispatch` базы затирает его ошибкой ТРАНСПОРТА (и наоборот). Это
        // видно прямо в голдене: после успешного ответа с непустым
        // `remote_search.error` итоговый `errorText` пуст (затёрт), а после
        // ошибки демона в нём лежит «источник упал» — то есть плашка
        // источника показывает сообщение транспорта. В порте это два разных
        // поля.
        //
        // Поэтому сверяется не конечное значение, а СЛЕД: значения в моменты
        // `stateChanged`, на котором висит notify свойства, — ровно то, что
        // перечитывает привязка QML и видит покупатель. Он обязан совпадать
        // посимвольно.
        assert_eq!(
            harness.error_text_trace,
            golden::strings_at(want, "errorTextTrace"),
            "{name}: след текста ошибки источника"
        );
        assert_eq!(
            harness.search.offset as i64,
            golden::i64_at(want, "offset"),
            "{name}: номер страницы"
        );
        assert_eq!(
            harness.search.no_more,
            golden::bool_at(want, "noMore"),
            "{name}: исчерпание выдачи"
        );
        assert_eq!(
            golden::rows_json(harness.rows.rows()),
            want["rows"],
            "{name}: строки выдачи"
        );

        let want = golden::final_of(case, "details");
        assert_eq!(
            harness.details.base.status as i64,
            golden::i64_at(want, "status"),
            "{name}: статус деталей"
        );
        assert_eq!(
            harness.details.base.error,
            golden::str_at(want, "error"),
            "{name}: ошибка деталей"
        );
        assert_eq!(
            harness.details.source_id,
            golden::str_at(want, "sourceId"),
            "{name}: источник деталей"
        );
        assert_eq!(
            harness.details.item_id,
            golden::str_at(want, "itemId"),
            "{name}: предмет деталей"
        );
        assert_eq!(
            harness.details.description,
            golden::str_at(want, "description"),
            "{name}: описание"
        );
        assert_eq!(
            harness.details.size,
            golden::str_at(want, "size"),
            "{name}: размер"
        );
        assert_eq!(
            harness.details.width as i64,
            golden::i64_at(want, "width"),
            "{name}: ширина"
        );
        assert_eq!(
            harness.details.height as i64,
            golden::i64_at(want, "height"),
            "{name}: высота"
        );
        assert_eq!(
            harness.details.tags,
            golden::strings_at(want, "tags"),
            "{name}: теги деталей"
        );
        assert_eq!(
            harness.details.web_url,
            golden::str_at(want, "webUrl"),
            "{name}: ссылка на страницу"
        );

        let want = golden::final_of(case, "download");
        assert_eq!(
            harness.download.base.status as i64,
            golden::i64_at(want, "status"),
            "{name}: статус скачивания"
        );
        assert_eq!(
            harness.download.base.error,
            golden::str_at(want, "error"),
            "{name}: ошибка скачивания"
        );

        harness.availability.sources.len()
    }

    #[test]
    fn request_bytes_are_the_real_protocol() {
        // Сигнал уносит БАЙТЫ, а не намерение: сверяем, что собранный кадр
        // разбирается обратно в тот же запрос. Иначе «поиск отправлен» мог
        // бы значить «отправлен пустой кадр».
        let cases = [
            Request::Availability,
            Request::Search {
                source_id: "wallhaven".into(),
                query: "лес".into(),
                sort_key: "toplist".into(),
                page: 2,
                required_tags: vec!["anime".into(), "CG".into()],
            },
            Request::Details {
                source_id: "wallhaven".into(),
                id: "a1".into(),
            },
            Request::Download {
                source_id: "wallhaven".into(),
                id: "a1".into(),
            },
            Request::Uninstall {
                source_id: "wallhaven".into(),
                id: "a1".into(),
            },
        ];
        for request in cases {
            let bytes = request.encode(7);
            assert_eq!(
                Request::decode(&bytes).as_ref(),
                Some(&request),
                "кадр {request:?} не разобрался обратно"
            );
        }
    }

    #[test]
    fn downloaded_item_becomes_acquired_row() {
        // v0.2.6: булев downloaded → состояние приобретения (3 — есть).
        let row = row_from_item(&pb::RemoteItem {
            id: "a1".into(),
            source_id: "wallhaven".into(),
            downloaded: true,
            ..Default::default()
        });
        assert_eq!(row.acquisition_state, 3);
        let row = row_from_item(&pb::RemoteItem {
            downloaded: false,
            ..Default::default()
        });
        assert_eq!(row.acquisition_state, 0);
    }

    /// Сторож самой сверки: `strip_message_maps` новые поля СНИМАЕТ, значит
    /// их пропажу она заметить не может в принципе. Ловить пропажу — работа
    /// `message_map_problems`, и вот доказательство, что она её ловит, а не
    /// возвращает пустой список на что угодно.
    ///
    /// ⚠️ Отдельная статья — `status.value`: подпись для человека там
    /// переведена намеренно («Вход не выполнен»), а машинный ключ живёт
    /// рядом в `valueText`, и страница «Источники» переводит именно его
    /// (`W.I18n.tr(modelData.valueText)`). Потеря ключа снаружи выглядит
    /// ровно как удачный перевод, поэтому она проверяется здесь поимённо.
    #[test]
    fn message_map_problems_catches_lost_keys() {
        let good = serde_json::json!([{
            "id": "wallhaven",
            "displayNameText": { "pluginId": "", "msgid": "", "arguments": [] },
            "remoteHintText": { "pluginId": "", "msgid": "", "arguments": [] },
            "sorts": [{ "key": "toplist", "label": "Toplist",
                        "labelText": { "pluginId": "", "msgid": "", "arguments": [] } }],
            "filters": [{ "id": "categories", "options": [
                { "value": "CG", "label": "Комп. графика", "labelText": "Комп. графика" }
            ] }],
            "status": [{ "id": "account", "value": "Вход не выполнен",
                         "valueText": "Not signed in" }],
        }]);
        assert_eq!(
            golden::message_map_problems(&good),
            Vec::<String>::new(),
            "целое описание источника не должно вызывать нареканий"
        );

        // Каждое поле выбивается по отдельности: общая проверка «хоть что-то
        // нашлось» прощала бы пропажу любого одного.
        for (path, key) in [
            ("/0/displayNameText", "карта имени источника"),
            ("/0/remoteHintText", "карта подписи кнопки"),
            ("/0/sorts/0/labelText", "карта подписи сортировки"),
            ("/0/filters/0/options/0/value", "машинный ключ варианта фильтра"),
            ("/0/filters/0/options/0/labelText", "переводимая подпись варианта"),
            ("/0/status/0/valueText", "машинный ключ строки состояния"),
        ] {
            let mut broken = good.clone();
            let (parent, leaf) = path.rsplit_once('/').expect("путь");
            broken
                .pointer_mut(parent)
                .and_then(Value::as_object_mut)
                .expect("родитель")
                .remove(leaf);
            assert!(
                !golden::message_map_problems(&broken).is_empty(),
                "пропажа «{key}» ({path}) прошла незамеченной"
            );
        }

        // Источников нет вовсе — это законно (демон ответил ошибкой).
        assert_eq!(
            golden::message_map_problems(&serde_json::json!([])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn record_kinds_are_distinguishable() {
        // Записи журнала обязаны различаться по типу: иначе сверка
        // «сигнал запроса» и «сигнал модели» слиплась бы.
        let query = RecordKind::Query(Event::Querying);
        let model = RecordKind::Model(crate::remote_list_model::ModelEvent::CountChanged);
        assert_ne!(query, model);
    }
}
