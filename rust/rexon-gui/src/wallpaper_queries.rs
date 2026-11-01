// WallpaperListQuery — запрос библиотеки обоей страницы «Обои».
//
// Самая тяжёлая часть переноса страницы «Обои»:
// именно здесь в свойства запроса приезжают Qt-Protobuf-ГАДЖЕТЫ
// (`sorts`/`filters`/`filterLogics`), которых не умеет ни Python (PySide6
// сегфолтится на маршалинге), ни cxx-qt (`Q_GADGET` в 0.9 нет).
//
// Как это решено. Значения разбирает обобщённый C++-шим мета-рефлексии
// `rexon-qt-filter/cpp/rexon_gadget_shim.cpp` — тот же самый, на котором
// стоит модель редактора фильтров; он один на проект (правило #12), просто
// подключён теперь и сюда. Шим берёт метатип В РАНТАЙМЕ и отдаёт наружу
// JSON, а решения по значениям принимает ядро (`rexon_core::we_wallpaper`,
// `we_settings`). Ни одного protobuf-типа по имени здесь не линкуется.
//
// Что делает этот файл и чего НЕ делает. Он не считает ни одного правила:
// сравнение свойств, порядок сигналов, сборка кадра `wallpaper_list` и
// разбор ответа — всё в ядре, ровно как было у питоновского шима. Здесь
// только Qt-оболочка: свойства, сигналы, владение моделью и дебаунс.
//
// Транспорт — общий шов партий 2-3 (`we_transport`): кадр уходит наружу
// сигналом, ответ приходит инвокаблом `deliverResponse`.

/// Мост cxx-qt: запросы страницы «Обои».
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
    }

    // Шим мета-рефлексии гаджетов (`rexon-qt-filter/cpp`). Живому Rexon
    // эти входы отсюда не нужны — их зовёт модель редактора фильтров из
    // своей сборки. Здесь они ради стенда (src/wallpaper_page_selftest.rs):
    // он строит правила на настоящих гаджетах ТЕМИ ЖЕ вызовами, что модель,
    // и проверяет, что до ядра они доезжают целыми.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("rexon_gadget_shim.h");
        fn rexon_default_gadget(type_name: &str) -> QVariant;
        fn rexon_coerce_gadget(type_name: &str, value: &QVariant) -> QVariant;
        fn rexon_gadget_set_by_role(gadget: &QVariant, role_index: i32, value: &QVariant)
            -> QVariant;
        fn rexon_gadget_set_int(gadget: &QVariant, prop: &str, value: i32) -> QVariant;
        fn rexon_role_names(type_name: &str) -> QHash_i32_QByteArray;
    }

    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Создать модель библиотеки как ребёнка запроса.
        ///
        /// # Safety
        /// `parent` — живой QObject (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_list_model(parent: *mut QObject) -> *mut QObject;

        /// Одноразовый таймер отложенной перезагрузки (100 мс оригинала).
        ///
        /// # Safety
        /// `owner` — живой QObject с инвокаблом `reload()`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` — таймер, созданный `rexon_new_reload_debounce`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_wallpaper_debounce(timer: *mut QObject);

        // ── Фабрики стенда голдена (src/wallpaper_page_selftest.rs) ──────
        //
        // В живом Rexon все восемь объектов страницы создаёт QML, и фабрика
        // ему не нужна. Стенду нужны те же самые классы без окна и без QML —
        // ровно как это сделано для запросов Discover.
        //
        // # Safety
        // `parent` — либо nullptr, либо живой QObject (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_list_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_get_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_apply_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_remove_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_scan_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_property_set_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_layout_set_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_apply_via_portal_query(parent: *mut QObject)
            -> *mut QObject;
        /// waywallen 0.4.4: «скрыть обои» (проверка v0.4.4 того же стенда).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_wallpaper_hide_query(parent: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── база Query ────────────────────────────────────────────────────
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        // ── свойства-параметры ────────────────────────────────────────────
        #[qproperty(QString, wp_type, cxx_name = "wpType", READ = wp_type,
                    WRITE = set_wp_type, NOTIFY = wp_type_changed)]
        #[qproperty(QString, search_text, cxx_name = "searchText",
                    READ = search_text, WRITE = set_search_text,
                    NOTIFY = search_text_changed)]
        #[qproperty(QVariant, sorts, READ = sorts, WRITE = set_sorts,
                    NOTIFY = sorts_changed)]
        #[qproperty(QVariant, filters, READ = filters, WRITE = set_filters,
                    NOTIFY = filters_changed)]
        #[qproperty(QVariant, filter_logics, cxx_name = "filterLogics",
                    READ = filter_logics, WRITE = set_filter_logics,
                    NOTIFY = filter_logics_changed)]
        #[qproperty(QStringList, skip_types, cxx_name = "skipTypes",
                    READ = skip_types, WRITE = set_skip_types,
                    NOTIFY = skip_types_changed)]
        #[qproperty(QStringList, filter_tags, cxx_name = "filterTags",
                    READ = filter_tags, WRITE = set_filter_tags,
                    NOTIFY = filter_tags_changed)]
        #[qproperty(QStringList, skip_content_ratings, cxx_name = "skipContentRatings",
                    READ = skip_content_ratings, WRITE = set_skip_content_ratings,
                    NOTIFY = skip_content_ratings_changed)]
        /// waywallen 0.4.4: какие обои показывать по признаку «скрыта»
        /// (`WallpaperHiddenFilter`: 0 прятать, 1 только скрытые, 2 все).
        #[qproperty(i32, hidden_filter, cxx_name = "hiddenFilter",
                    READ = hidden_filter, WRITE = set_hidden_filter,
                    NOTIFY = hidden_filter_changed)]
        /// `limit` базы `QueryList` — размер страницы; 0 (умолчание) —
        /// страниц нет, демон отдаёт весь список. Пишут «Скрытые обои»
        /// (`limit: 100`) и предпросмотр умного плейлиста (`limit: 50`,
        /// waywallen 0.5.0). Когда смена перезагружает список — см.
        /// `rexon_core::we_wallpaper::set_limit`.
        #[qproperty(i32, limit, READ = limit, WRITE = set_limit, NOTIFY = limit_changed)]
        /// `offset` базы: номер последней загруженной страницы.
        #[qproperty(i32, offset, READ = offset, WRITE = set_offset, NOTIFY = offset_changed)]
        /// `noMore` базы: дальше страниц нет.
        #[qproperty(bool, no_more, cxx_name = "noMore", READ = no_more,
                    WRITE = set_no_more, NOTIFY = no_more_changed)]
        /// waywallen 0.5.0: выборка — непрозрачное значение (в JS —
        /// `ArrayBuffer`), которое QML только хранит и отдаёт обратно в
        /// `restoreSelection`/`matchesSelection` (и `saveSmart`). Вид — см.
        /// `rexon_core::we_wallpaper::encode_selection`.
        #[qproperty(QVariant, selection, READ = selection, NOTIFY = selection_changed)]
        /// waywallen 0.5.0: номера скрытых библиотек (`QList<qlonglong>`
        /// оригинала). Наружу — список чисел-double: 64-битное целое в QML
        /// не выставляется (см. `qvalue.rs`). Сеттер нормализует список —
        /// по возрастанию, без повторов.
        #[qproperty(QVariant, hidden_library_ids, cxx_name = "hiddenLibraryIds",
                    READ = hidden_library_ids, WRITE = set_hidden_library_ids,
                    NOTIFY = hidden_library_ids_changed)]
        // ── свойства-результаты ───────────────────────────────────────────
        #[qproperty(bool, has_active_filters, cxx_name = "hasActiveFilters",
                    READ = has_active_filters, NOTIFY = has_active_filters_changed)]
        /// waywallen 0.5.0: фильтры, КРОМЕ скрытых библиотек (подсветка
        /// кнопки «Фильтры» на странице «Обои»).
        #[qproperty(bool, has_other_filters, cxx_name = "hasOtherFilters",
                    READ = has_other_filters, NOTIFY = has_other_filters_changed)]
        #[qproperty(i32, total, READ = total, NOTIFY = total_changed)]
        /// Модель библиотеки. Имя свойства — `data`, как у оригинала:
        /// вендорный QML пишет `model: wallpaperQuery.data`. CONSTANT —
        /// модель создаётся в пост-конструкции и живёт, пока жив запрос.
        #[qproperty(*mut QObject, model, cxx_name = "data", READ = data, CONSTANT)]
        type WallpaperListQuery = super::WallpaperListQueryRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperListQuery>, error: QString);

        #[qsignal]
        #[cxx_name = "wpTypeChanged"]
        fn wp_type_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "searchTextChanged"]
        fn search_text_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "sortsChanged"]
        fn sorts_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filtersChanged"]
        fn filters_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filterLogicsChanged"]
        fn filter_logics_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "skipTypesChanged"]
        fn skip_types_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "filterTagsChanged"]
        fn filter_tags_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "skipContentRatingsChanged"]
        fn skip_content_ratings_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "hiddenFilterChanged"]
        fn hidden_filter_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "limitChanged"]
        fn limit_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "offsetChanged"]
        fn offset_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "noMoreChanged"]
        fn no_more_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0: выборка сменилась — `invalidateList` оригинала
        /// шлёт его на каждую смену свойства выборки и `limit`.
        #[qsignal]
        #[cxx_name = "selectionChanged"]
        fn selection_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0.
        #[qsignal]
        #[cxx_name = "hiddenLibraryIdsChanged"]
        fn hidden_library_ids_changed(self: Pin<&mut WallpaperListQuery>);
        /// waywallen 0.5.0.
        #[qsignal]
        #[cxx_name = "hasOtherFiltersChanged"]
        fn has_other_filters_changed(self: Pin<&mut WallpaperListQuery>);
        /// Один сигнал на оба состояния фильтра — он же и запускает
        /// перезагрузку списка (`connect_requet_reload` оригинала).
        #[qsignal]
        #[cxx_name = "filterStateChanged"]
        fn filter_state_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "hasActiveFiltersChanged"]
        fn has_active_filters_changed(self: Pin<&mut WallpaperListQuery>);
        #[qsignal]
        #[cxx_name = "totalChanged"]
        fn total_changed(self: Pin<&mut WallpaperListQuery>);
        /// Служебный сигнал шима (свойства `items` у оригинала нет): набор
        /// обоей сменился, модель пересобрана.
        ///
        /// ⚠️ Порядок: модель перестраивается ДО этого сигнала — у
        /// питоновского моста пересборку вешал конструктор запроса, то есть
        /// она стояла первой в цепочке подписчиков. QML на `onItemsChanged`
        /// обязан читать уже НОВЫЕ строки.
        #[qsignal]
        #[cxx_name = "itemsChanged"]
        fn items_changed(self: Pin<&mut WallpaperListQuery>);

        /// Кадр наружу — общий шов партий 2-3.
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(
            self: Pin<&mut WallpaperListQuery>,
            token: i64,
            field: QString,
            request: QByteArray,
        );
    }

    extern "RustQt" {
        fn status(self: &WallpaperListQuery) -> i32;
        fn error(self: &WallpaperListQuery) -> QString;
        fn querying(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperListQuery>, value: bool);

        #[cxx_name = "wpType"]
        fn wp_type(self: &WallpaperListQuery) -> QString;
        #[cxx_name = "setWpType"]
        fn set_wp_type(self: Pin<&mut WallpaperListQuery>, value: QString);
        #[cxx_name = "searchText"]
        fn search_text(self: &WallpaperListQuery) -> QString;
        #[cxx_name = "setSearchText"]
        fn set_search_text(self: Pin<&mut WallpaperListQuery>, value: QString);

        fn sorts(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setSorts"]
        fn set_sorts(self: Pin<&mut WallpaperListQuery>, value: QVariant);
        fn filters(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setFilters"]
        fn set_filters(self: Pin<&mut WallpaperListQuery>, value: QVariant);
        #[cxx_name = "filterLogics"]
        fn filter_logics(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setFilterLogics"]
        fn set_filter_logics(self: Pin<&mut WallpaperListQuery>, value: QVariant);

        #[cxx_name = "skipTypes"]
        fn skip_types(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setSkipTypes"]
        fn set_skip_types(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "filterTags"]
        fn filter_tags(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setFilterTags"]
        fn set_filter_tags(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "skipContentRatings"]
        fn skip_content_ratings(self: &WallpaperListQuery) -> QStringList;
        #[cxx_name = "setSkipContentRatings"]
        fn set_skip_content_ratings(self: Pin<&mut WallpaperListQuery>, value: QStringList);
        #[cxx_name = "hiddenFilter"]
        fn hidden_filter(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setHiddenFilter"]
        fn set_hidden_filter(self: Pin<&mut WallpaperListQuery>, value: i32);
        fn limit(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setLimit"]
        fn set_limit(self: Pin<&mut WallpaperListQuery>, value: i32);
        fn offset(self: &WallpaperListQuery) -> i32;
        #[cxx_name = "setOffset"]
        fn set_offset(self: Pin<&mut WallpaperListQuery>, value: i32);
        #[cxx_name = "noMore"]
        fn no_more(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "setNoMore"]
        fn set_no_more(self: Pin<&mut WallpaperListQuery>, value: bool);
        fn selection(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "hiddenLibraryIds"]
        fn hidden_library_ids(self: &WallpaperListQuery) -> QVariant;
        #[cxx_name = "setHiddenLibraryIds"]
        fn set_hidden_library_ids(self: Pin<&mut WallpaperListQuery>, value: QVariant);

        #[cxx_name = "hasActiveFilters"]
        fn has_active_filters(self: &WallpaperListQuery) -> bool;
        #[cxx_name = "hasOtherFilters"]
        fn has_other_filters(self: &WallpaperListQuery) -> bool;
        fn total(self: &WallpaperListQuery) -> i32;
    }

    // ── Инвокаблы ─────────────────────────────────────────────────────────
    // unsafe-блок: геттер свойства `data` отдаёт голый указатель.
    unsafe extern "RustQt" {
        fn data(self: &WallpaperListQuery) -> *mut QObject;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperListQuery>);

        /// `delayReload` базы `Query` (qextra): перезагрузка через тот же
        /// дебаунс 100 мс — подряд идущие просьбы склеиваются в один запрос.
        /// Зовёт страница «Скрытые обои» (waywallen 0.4.4).
        #[qinvokable]
        #[cxx_name = "delayReload"]
        fn delay_reload(self: Pin<&mut WallpaperListQuery>);

        /// `replaceFilterState` оригинала: правила и логика групп меняются
        /// разом — один `filterStateChanged` и одна перезагрузка вместо
        /// двух. Возвращает, изменилось ли что-нибудь.
        #[qinvokable]
        #[cxx_name = "replaceFilterState"]
        fn replace_filter_state(
            self: Pin<&mut WallpaperListQuery>,
            filters: QVariant,
            logics: QVariant,
        ) -> bool;

        /// waywallen 0.5.0: глазик библиотеки — скрыть/показать её обои.
        /// Номер — double (64-битное целое в QML не выставляется).
        #[qinvokable]
        #[cxx_name = "toggleLibraryHidden"]
        fn toggle_library_hidden(self: Pin<&mut WallpaperListQuery>, id: f64);

        /// waywallen 0.5.0: разложить выборку по свойствам. Не выборка —
        /// ничего не делать.
        #[qinvokable]
        #[cxx_name = "restoreSelection"]
        fn restore_selection(self: Pin<&mut WallpaperListQuery>, value: &QVariant);

        /// waywallen 0.5.0: совпадает ли текущая выборка с сохранённой.
        #[qinvokable]
        #[cxx_name = "matchesSelection"]
        fn matches_selection(self: &WallpaperListQuery, value: &QVariant) -> bool;

        /// `fetchMore(qint32)` базы `QueryList`: догрузить следующую
        /// страницу. Аргумент оригинал не читает (размер — `limit`).
        #[qinvokable]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut WallpaperListQuery>, count: i32);

        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperListQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Constructor<()> for WallpaperListQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QStringList, QVariant};
use serde_json::Value;

use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR};
use rexon_core::we_settings::VMap;
use rexon_core::we_wallpaper;

use crate::qvalue::{json_to_variant, variant_to_json};
use crate::we_transport;

/// Дебаунс перезагрузки — 100 мс, как у оригинала (qextra query.cpp:16).
const DEBOUNCE_MS: i32 = 100;

/// Какие свойства запускают отложенную перезагрузку списка — список из
/// конструктора оригинала (`connect_requet_reload`). Наружу он открыт не для
/// красоты: тем же списком пользуется реплей голдена (wallpaper_page_golden),
/// и второй его копии в проекте быть не должно.
pub const LIST_RELOAD_ON: &[&str] = &[
    "wpType",
    "filterState",
    "sorts",
    "searchText",
    "skipTypes",
    "filterTags",
    "skipContentRatings",
    // waywallen 0.4.4: `connect_requet_reload(&hiddenFilterChanged)`.
    "hiddenFilter",
    // waywallen 0.5.0: `hiddenLibraryIdsChanged → invalidateList`.
    "hiddenLibraryIds",
];

/// Чей ответ ждёт токен кадра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// `reload()` — список целиком (первая страница).
    Reload,
    /// `fetchMore()` — страница с этим номером, в хвост.
    Page(i32),
}

/// Свойство `data` отдаёт модель; ставится в пост-конструкции.
pub struct WallpaperListQueryRust {
    base: QueryState,
    data: we_wallpaper::WallpaperListData,
    /// Токены кадров ТЕКУЩЕГО поколения (`m_generation` оригинала): каждый
    /// `reload` и каждая смена выборки сносят их, и ответ на устаревший
    /// запрос молча отбрасывается — старая выборка не перебьёт новую, а
    /// страница старой выборки не ляжет в хвост новой.
    pending: std::collections::BTreeMap<i64, Pending>,
    model: *mut qobject::QObject,
    debounce: *mut qobject::QObject,
    /// Подписка на `reqFetchMore` модели (живёт, пока жив запрос).
    fetch_guard: Option<cxx_qt::QMetaObjectConnectionGuard>,
}

impl Default for WallpaperListQueryRust {
    fn default() -> Self {
        Self {
            base: QueryState::default(),
            data: we_wallpaper::WallpaperListData::default(),
            pending: std::collections::BTreeMap::new(),
            model: core::ptr::null_mut(),
            debounce: core::ptr::null_mut(),
            fetch_guard: None,
        }
    }
}

/// Указатель на запрос для замыкания подписки (см. discover_queries.rs).
struct OwnerPtr(*mut qobject::WallpaperListQuery);
// SAFETY: сигнал модели приходит в потоке GUI, в котором живёт и запрос;
// Send нужен только сигнатуре `on_req_fetch_more`.
unsafe impl Send for OwnerPtr {}

/// Указатель на сам объект как на QObject.
fn self_ptr<T>(object: Pin<&mut T>) -> *mut qobject::QObject {
    let raw: *mut T = unsafe { object.get_unchecked_mut() };
    raw.cast()
}

/// Модель как изменяемая ссылка (см. wallpaper_select_storage::model_pin).
///
/// # Safety
/// `ptr` — либо nullptr, либо живая `WallpaperListModel`.
unsafe fn model_pin<'a>(
    ptr: *mut qobject::QObject,
) -> Option<Pin<&'a mut crate::wallpaper_list_model::qobject::WallpaperListModel>> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe {
        Pin::new_unchecked(
            &mut *ptr.cast::<crate::wallpaper_list_model::qobject::WallpaperListModel>(),
        )
    })
}

/// Список правил из QML → JSON-значение для ядра.
///
/// Элементы приходят Qt-Protobuf-гаджетами (их пишет Rust-модель редактора
/// фильтров) либо обычными JS-объектами (карты из настроек демона). Оба
/// вида разбирает ОБЩИЙ конвертер `qvalue::variant_to_json` — своего
/// списочного шортката у этой страницы больше нет (правило #12): гаджеты
/// понимает каждый путь «QML → ядро», а не только этот.
///
/// Не список — пустой массив: ядро трактует его как «правил нет», и это
/// ровно то, что делал питоновский `_rules_plain`.
fn rules_value(value: &QVariant) -> Value {
    match variant_to_json(value) {
        list @ Value::Array(_) => list,
        _ => Value::Array(Vec::new()),
    }
}

fn strings_value(list: &QStringList) -> Value {
    Value::Array(
        list.iter()
            .map(|s| Value::String(s.to_string()))
            .collect::<Vec<_>>(),
    )
}

/// Байты выборки из значения QML. Из JS она возвращается `ArrayBuffer`'ом,
/// то есть `QByteArray`; `undefined`/`null` в `QByteArray` не приводятся, и
/// это «не выборка». Что байты — именно выборка, решает ядро по метке
/// (`we_wallpaper::decode_selection`). Тем же путём выборку принимает
/// `PlaylistMutationQuery.saveSmart`.
pub(crate) fn selection_bytes(value: &QVariant) -> Option<Vec<u8>> {
    value.value::<QByteArray>().map(|bytes| bytes.as_slice().to_vec())
}

fn maps_to_variant(maps: &[VMap]) -> QVariant {
    json_to_variant(&Value::Array(
        maps.iter().map(|m| Value::Object(m.clone())).collect(),
    ))
}

impl cxx_qt::Initialize for qobject::WallpaperListQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        // Модель создаётся в пост-конструкции, а не лениво в геттере: QML
        // читает `data` при первой же привязке.
        let model = unsafe { qobject::rexon_new_wallpaper_list_model(owner) };
        let timer = unsafe { qobject::rexon_new_wallpaper_debounce(owner, DEBOUNCE_MS) };
        {
            let mut rust = self.as_mut().rust_mut();
            rust.model = model;
            rust.debounce = timer;
        }
        // `try_connect_fetch_more` QExtra: модель просит следующую страницу.
        // Указатель на владельца безопасен ровно потому, что модель — его
        // ребёнок и не переживёт запрос.
        let query = OwnerPtr(unsafe { self.as_mut().get_unchecked_mut() });
        if let Some(model) = unsafe { model_pin(model) } {
            let guard = model.on_req_fetch_more(move |_model, rows| {
                // Захват ВСЕЙ обёртки, а не поля (редакция 2021).
                let owner = &query;
                let query = unsafe { Pin::new_unchecked(&mut *owner.0) };
                query.fetch_more(rows);
            });
            self.as_mut().rust_mut().fetch_guard = Some(guard);
        }
    }
}

impl qobject::WallpaperListQuery {
    // ── база Query ────────────────────────────────────────────────────────

    pub fn status(&self) -> i32 {
        self.base.status
    }

    pub fn error(&self) -> QString {
        QString::from(&self.base.error)
    }

    pub fn querying(&self) -> bool {
        self.base.status == we_query::STATUS_QUERYING
    }

    pub fn forward_error(&self) -> bool {
        self.base.forward_error
    }

    pub fn set_forward_error(mut self: Pin<&mut Self>, value: bool) {
        let signals = we_query::set_forward_error(&mut self.as_mut().rust_mut().base, value);
        self.dispatch(signals);
    }

    // ── свойства-параметры ────────────────────────────────────────────────

    pub fn wp_type(&self) -> QString {
        QString::from(&self.data.wp_type)
    }

    pub fn set_wp_type(self: Pin<&mut Self>, value: QString) {
        self.set_core_property("wpType", Value::String(value.to_string()));
    }

    pub fn search_text(&self) -> QString {
        QString::from(&self.data.search_text)
    }

    pub fn set_search_text(self: Pin<&mut Self>, value: QString) {
        self.set_core_property("searchText", Value::String(value.to_string()));
    }

    pub fn sorts(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .sorts
                .iter()
                .map(rexon_core::we_settings::sort_rule_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_sorts(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("sorts", rules);
    }

    pub fn filters(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .filters
                .iter()
                .map(rexon_core::we_settings::filter_rule_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_filters(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("filters", rules);
    }

    pub fn filter_logics(&self) -> QVariant {
        maps_to_variant(
            &self
                .data
                .filter_logics
                .iter()
                .map(rexon_core::we_settings::filter_logic_to_map)
                .collect::<Vec<_>>(),
        )
    }

    pub fn set_filter_logics(self: Pin<&mut Self>, value: QVariant) {
        let rules = rules_value(&value);
        self.set_core_property("filterLogics", rules);
    }

    pub fn skip_types(&self) -> QStringList {
        QStringList::from_iter(self.data.skip_types.iter().map(QString::from))
    }

    pub fn set_skip_types(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("skipTypes", list);
    }

    pub fn filter_tags(&self) -> QStringList {
        QStringList::from_iter(self.data.filter_tags.iter().map(QString::from))
    }

    pub fn set_filter_tags(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("filterTags", list);
    }

    pub fn skip_content_ratings(&self) -> QStringList {
        QStringList::from_iter(self.data.skip_content_ratings.iter().map(QString::from))
    }

    pub fn set_skip_content_ratings(self: Pin<&mut Self>, value: QStringList) {
        let list = strings_value(&value);
        self.set_core_property("skipContentRatings", list);
    }

    pub fn hidden_filter(&self) -> i32 {
        self.data.hidden_filter
    }

    pub fn set_hidden_filter(self: Pin<&mut Self>, value: i32) {
        self.set_core_property("hiddenFilter", Value::from(value));
    }

    pub fn limit(&self) -> i32 {
        self.data.limit
    }

    pub fn set_limit(mut self: Pin<&mut Self>, value: i32) {
        let status = self.base.status;
        let (signals, reload) = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::set_limit(&mut rust.data, value, status)
        };
        if reload {
            self.as_mut().rust_mut().pending.clear();
            self.as_mut().delay_reload();
        }
        self.dispatch(signals);
    }

    pub fn offset(&self) -> i32 {
        self.data.offset
    }

    pub fn set_offset(mut self: Pin<&mut Self>, value: i32) {
        if self.data.offset != value {
            self.as_mut().rust_mut().data.offset = value;
            self.offset_changed();
        }
    }

    pub fn no_more(&self) -> bool {
        self.data.no_more
    }

    pub fn set_no_more(mut self: Pin<&mut Self>, value: bool) {
        if self.data.no_more != value {
            self.as_mut().rust_mut().data.no_more = value;
            self.no_more_changed();
        }
    }

    pub fn selection(&self) -> QVariant {
        let bytes = we_wallpaper::encode_selection(&we_wallpaper::selection_of(&self.data));
        QVariant::from(&QByteArray::from(&bytes[..]))
    }

    pub fn restore_selection(mut self: Pin<&mut Self>, value: &QVariant) {
        let Some(bytes) = selection_bytes(value) else {
            return;
        };
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::restore_selection(&mut rust.data, &bytes)
        };
        self.dispatch(signals);
    }

    pub fn matches_selection(&self, value: &QVariant) -> bool {
        selection_bytes(value).is_some_and(|bytes| we_wallpaper::matches_selection(&self.data, &bytes))
    }

    pub fn hidden_library_ids(&self) -> QVariant {
        json_to_variant(&Value::Array(
            self.data
                .hidden_library_ids
                .iter()
                .map(|id| Value::from(crate::qvalue::i64_to_qml(*id)))
                .collect(),
        ))
    }

    pub fn set_hidden_library_ids(self: Pin<&mut Self>, value: QVariant) {
        // Не список (undefined из `global.wallpaperHiddenLibraryIds || []`
        // сюда не доходит, но null может) — ядро трактует как «скрытых нет».
        let list = variant_to_json(&value);
        self.set_core_property("hiddenLibraryIds", list);
    }

    pub fn toggle_library_hidden(mut self: Pin<&mut Self>, id: f64) {
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::toggle_library_hidden(&mut rust.data, crate::qvalue::qml_to_i64(id))
        };
        self.dispatch(signals);
    }

    pub fn has_other_filters(&self) -> bool {
        self.data.has_other_filters()
    }

    pub fn delay_reload(self: Pin<&mut Self>) {
        let timer = self.debounce;
        if !timer.is_null() {
            unsafe { qobject::rexon_restart_wallpaper_debounce(timer) };
        }
    }

    pub fn has_active_filters(&self) -> bool {
        self.data.has_active_filters()
    }

    pub fn total(&self) -> i32 {
        self.data.total
    }

    pub fn data(&self) -> *mut qobject::QObject {
        self.model
    }

    // ── действия ──────────────────────────────────────────────────────────

    pub fn reload(mut self: Pin<&mut Self>) {
        // Новое поколение: ответы прежних запросов больше не наши, страницы
        // сброшены (`reload` оригинала: `++m_generation`, `setOffset(0)`,
        // `setNoMore(false)`).
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            rust.pending.clear();
            we_wallpaper::begin_reload(&mut rust.data)
        };
        self.as_mut().dispatch(signals);
        // Нет соединения — запрос сразу в Error, кадр не уходит. Решение и
        // сигналы считает ядро, ветка та же, что была у Python `_begin`.
        let connected = crate::discover_queries::connected();
        let (send, signals) = we_query::begin_call(&mut self.as_mut().rust_mut().base, connected);
        self.as_mut().dispatch(signals);
        if !send {
            return;
        }
        let payload = we_wallpaper::encode_list_request(&self.data);
        self.send_list(payload, Pending::Reload);
    }

    pub fn fetch_more(mut self: Pin<&mut Self>, _count: i32) {
        let Some(payload) = we_wallpaper::encode_fetch_more_request(&self.data, self.base.status)
        else {
            return;
        };
        let connected = crate::discover_queries::connected();
        let (send, signals) = we_query::begin_call(&mut self.as_mut().rust_mut().base, connected);
        self.as_mut().dispatch(signals);
        if !send {
            return;
        }
        let page = self.data.offset + 1;
        self.send_list(payload, Pending::Page(page));
    }

    /// Отправить кадр `wallpaper_list` и запомнить, чей ответ ждём.
    fn send_list(mut self: Pin<&mut Self>, payload: Vec<u8>, kind: Pending) {
        // Ядро собирает payload, транспорту нужен ПОЛНЫЙ кадр Request с уже
        // проставленным `request_id`: счётчик один на процесс.
        let token = crate::discover_queries::next_token();
        let Ok(bytes) =
            rexon_core::we_client::request_bytes(token as u64, "wallpaper_list", &payload)
        else {
            debug_assert!(false, "поле wallpaper_list неизвестно протоколу");
            return;
        };
        self.as_mut().rust_mut().pending.insert(token, kind);
        let owner = self_ptr(self.as_mut());
        unsafe { we_transport::remember(token, owner.cast()) };
        self.as_mut().send_request(
            token,
            QString::from("wallpaper_list"),
            QByteArray::from(&bytes[..]),
        );
        we_transport::send(token, "wallpaper_list", &bytes);
    }

    pub fn replace_filter_state(mut self: Pin<&mut Self>, filters: QVariant, logics: QVariant) -> bool {
        let filters = rules_value(&filters);
        let logics = rules_value(&logics);
        let (changed, signals) = {
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::replace_filter_state(&mut rust.data, &filters, &logics)
        };
        self.dispatch(signals);
        changed
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(kind) = self.as_mut().rust_mut().pending.remove(&token) else {
            // Ответ не наш, устаревшего поколения или уже обработан —
            // молча дропаем.
            return;
        };
        // `t->setHasMore(false)` оригинала ДО правки строк: сетка спрашивает
        // `canFetchMore` прямо из сброса/вставки, и старый флаг послал бы
        // догрузку посреди разбора ответа.
        if let Some(model) = unsafe { model_pin(self.model) } {
            model.set_page_has_more(false);
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            // Разъять заимствование руками: `&mut rust.base` и
            // `&mut rust.data` — два поля одной структуры, но через
            // Pin-обёртку компилятор этого не видит.
            let rust = &mut *rust;
            match kind {
                Pending::Reload => we_wallpaper::apply_wallpaper_list(
                    &mut rust.base,
                    &mut rust.data,
                    frame.as_slice(),
                ),
                Pending::Page(page) => we_wallpaper::apply_wallpaper_page(
                    &mut rust.base,
                    &mut rust.data,
                    frame.as_slice(),
                    page,
                ),
            }
        };
        match signals {
            Ok(signals) => {
                self.as_mut().dispatch(signals);
                let more = !self.data.no_more;
                if let Some(model) = unsafe { model_pin(self.model) } {
                    model.set_page_has_more(more);
                }
            }
            Err(_) => {
                // Демон может быть новее нас — падать посреди приёма нельзя.
                let signals = {
                    let mut rust = self.as_mut().rust_mut();
                    let mut out = we_query::set_error(&mut rust.base, &rexon_core::i18n::tr("could not parse the daemon response"));
                    out.extend(we_query::set_status(&mut rust.base, STATUS_ERROR));
                    out
                };
                self.dispatch(signals);
            }
        }
    }

    // ── внутреннее ────────────────────────────────────────────────────────

    /// Записать свойство через ядро: сравнение и сигналы считает оно.
    pub fn set_core_property(mut self: Pin<&mut Self>, name: &str, value: Value) {
        let Some(signals) = ({
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::set_property(&mut rust.data, name, &value)
        }) else {
            debug_assert!(false, "нет свойства {name} у WallpaperListQuery");
            return;
        };
        self.dispatch(signals);
    }

    /// Разослать сигналы, посчитанные ядром, и выполнить их побочные
    /// действия: пересборку модели и отложенную перезагрузку.
    ///
    /// Значения читаются ЖИВЫЕ (из состояния), а не запомненные при
    /// планировании: обработчик в QML читает свойство и обязан увидеть
    /// новое значение.
    pub fn dispatch(mut self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        let mut reload = false;
        for signal in signals {
            match signal {
                QuerySignal::StatusChanged(v) => self.as_mut().status_changed(v),
                QuerySignal::QueryingChanged => self.as_mut().querying_changed(),
                QuerySignal::ErrorChanged => self.as_mut().error_changed(),
                QuerySignal::ForwardErrorChanged => self.as_mut().forward_error_changed(),
                QuerySignal::Finished => self.as_mut().finished(),
                QuerySignal::ErrorOccurred(text) => {
                    self.as_mut().error_occurred(QString::from(&text));
                }
                QuerySignal::AppErrorOccurred(text) => {
                    self.as_mut().app_error_occurred(QString::from(&text));
                }
                QuerySignal::Property(name) => {
                    self.as_mut().emit_property(name);
                    if LIST_RELOAD_ON.contains(&name) {
                        reload = true;
                        // `invalidateList` оригинала подписан на сигнал
                        // свойства и срабатывает СРАЗУ за ним:
                        // `selectionChanged`, сброс страниц, отмена запроса
                        // в полёте (новое поколение).
                        let follow = {
                            let mut rust = self.as_mut().rust_mut();
                            rust.pending.clear();
                            we_wallpaper::invalidate_list(&mut rust.data)
                        };
                        for next in follow {
                            if let QuerySignal::Property(next) = next {
                                self.as_mut().emit_property(next);
                            }
                        }
                    }
                }
            }
        }
        if reload {
            // Дебаунс настоящий: подряд идущие правки склеиваются в ОДИН
            // запрос, как у оригинала.
            let timer = self.debounce;
            if !timer.is_null() {
                unsafe { qobject::rexon_restart_wallpaper_debounce(timer) };
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "wpType" => self.as_mut().wp_type_changed(),
            "searchText" => self.as_mut().search_text_changed(),
            "sorts" => self.as_mut().sorts_changed(),
            "filters" => self.as_mut().filters_changed(),
            "filterLogics" => self.as_mut().filter_logics_changed(),
            "skipTypes" => self.as_mut().skip_types_changed(),
            "filterTags" => self.as_mut().filter_tags_changed(),
            "skipContentRatings" => self.as_mut().skip_content_ratings_changed(),
            "hiddenFilter" => self.as_mut().hidden_filter_changed(),
            "hiddenLibraryIds" => self.as_mut().hidden_library_ids_changed(),
            "hasOtherFilters" => self.as_mut().has_other_filters_changed(),
            "filterState" => self.as_mut().filter_state_changed(),
            "hasActiveFilters" => self.as_mut().has_active_filters_changed(),
            "total" => self.as_mut().total_changed(),
            "limit" => self.as_mut().limit_changed(),
            "offset" => self.as_mut().offset_changed(),
            "noMore" => self.as_mut().no_more_changed(),
            "selection" => self.as_mut().selection_changed(),
            "itemsAppended" => {
                // Догрузка: модели — только недостающий хвост, вставкой.
                // `itemsChanged` НЕ шлётся: у оригинала догрузка идёт
                // `t->insert`, а не пересборкой, и QML, который слушает
                // смену набора, не должен считать её новым списком.
                if let Some(model) = unsafe { model_pin(self.model) } {
                    let have = model.row_total();
                    let tail = self.data.items.get(have..).unwrap_or_default().to_vec();
                    model.append_rows(tail);
                }
            }
            "items" => {
                // Модель перестраивается ПЕРВОЙ — см. пояснение у сигнала.
                let rows = self.data.items.clone();
                if let Some(model) = unsafe { model_pin(self.model) } {
                    model.reset_rows(rows);
                }
                self.as_mut().items_changed();
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }
}
