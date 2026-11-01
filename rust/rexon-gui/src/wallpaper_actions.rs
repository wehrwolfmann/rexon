// Запросы-действия страницы «Обои»: применить, прочитать, удалить,
// пересканировать, записать свойство, задать раскладку, отдать через портал.
//
// Список обоей и выбор живут рядом (wallpaper_queries.rs,
// wallpaper_list_model.rs, wallpaper_select_storage.rs).
//
// ⚠️ Эталон — ОРИГИНАЛ (waywallen `ui/src/query/wallpaper_query.cpp`),
// а не наш Python. Порт чинит пять расхождений, которые перенос «как было»
// законсервировал бы (правило #16):
//
//   1. `WallpaperApplyQuery` ВЫБРАСЫВАЛ ответ демона (`lambda rsp: None`), а в
//      нём приезжает `renderer_id`. Свойство `rendererId` у нас поэтому было
//      вечно пустым, и `rendererIdChanged` не слался никогда.
//   2. `WallpaperRemoveQuery.remove()` не отсеивал пустые идентификаторы,
//      всегда слал списком и НИКОГДА не эмитил `removed(id)` — а вендорный
//      QML на него подписан (`WallpaperDetailPanel.qml:162`), то есть
//      обработчик был мёртв. Оригинал при единственном id кладёт его в
//      `wallpaper_id`, шлёт `removed` и только потом `removedMany`.
//   3. `removedMany` нёс длину ОТПРАВЛЕННОГО набора вместо `removed_count`
//      из ОТВЕТА демона: тост обещал «удалено 3», когда демон снёс одну.
//   4. `WallpaperRemoveQuery.reload()` не был переопределён вовсе — свойство
//      `wallpaperId` этого запроса было мёртвым. У оригинала
//      `reload() == remove([wallpaperId])`.
//   5. `WallpaperGetQuery` не проверял, тот ли предмет приехал. Оригинал
//      сверяет `m_wallpaper_id` с запрошенным и молча выходит при
//      расхождении — иначе при быстром переключении карточек панель деталей
//      показывает ЧУЖУЮ обоину.
//
// Каждая из пяти починок закрыта своим случаем реплея голдена
// (`wallpaper_page_golden.rs`, стенд `--wallpaper-selftest`): снятие любой из
// них немедленно валит сверку — проверено укусом.
//
// Сознательное отличие от оригинала, НЕ дефект: у `WallpaperPropertySetQuery`
// протокол v0.2.5 даёт `oneof operation { value | reset }`, которого у
// оригинала нет вовсе. Сброс свойства — наша ветка, она остаётся. А вот
// сигналы свойств разведены по одному на свойство, как у оригинала: наш
// Python схлопывал их все в общий `paramsChanged`.

// waywallen 0.4.4 добавил восьмой — `WallpaperHideQuery` (скрыть обои из
// библиотеки и вернуть их), и отражение `flip` у `WallpaperLayoutSetQuery`.

/// Мост cxx-qt: запросы-действия страницы «Обои».
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

    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Таймер отложенной перезагрузки под своим именем — третье
        /// объявление той же C++-функции в крейте дало бы duplicate symbol.
        ///
        /// # Safety
        /// `owner` — живой QObject с инвокаблом `reload()`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_action_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` — таймер, созданный `rexon_new_action_debounce`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_action_debounce(timer: *mut QObject);
    }

    // ── WallpaperApplyQuery ───────────────────────────────────────────────
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, wallpaper, READ = wallpaper, WRITE = set_wallpaper,
                    NOTIFY = wallpaper_changed)]
        /// Цели применения v0.3.5: список карт, в каждой ЛИБО `displayId`,
        /// ЛИБО `canvasId`. Пусто = «все выбираемые цели» (так трактует и
        /// демон). Свойство обязательное: `WallpaperDetailPanel.qml:343`
        /// присваивает его в обработчике кнопки Apply, а присвоение
        /// несуществующего свойства в QML — исключение, после которого
        /// обработчик обрывается и запрос не уходит вовсе.
        #[qproperty(QVariant, targets, READ = targets, WRITE = set_targets,
                    NOTIFY = targets_changed)]
        #[qproperty(QString, renderer_name, cxx_name = "rendererName", READ = renderer_name,
                    WRITE = set_renderer_name, NOTIFY = renderer_name_changed)]
        #[qproperty(QString, renderer_id, cxx_name = "rendererId", READ = renderer_id,
                    NOTIFY = renderer_id_changed)]
        /// Плейлисты, которые демон остановил этим применением: список
        /// карт `{playlistId, playlistName, allDisplays, displayIds}`.
        /// Панель деталей показывает по тосту на каждый
        /// (`WallpaperDetailPanel.qml`, `toastStoppedPlaylists`).
        #[qproperty(QVariant, stopped_playlists, cxx_name = "stoppedPlaylists",
                    READ = stopped_playlists, NOTIFY = stopped_playlists_changed)]
        type WallpaperApplyQuery = super::WallpaperApplyQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        #[qproperty(QVariant, wallpaper, READ = wallpaper, NOTIFY = wallpaper_changed)]
        type WallpaperGetQuery = super::WallpaperGetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        type WallpaperRemoveQuery = super::WallpaperRemoveQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type WallpaperScanQuery = super::WallpaperScanQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        #[qproperty(QString, property_key, cxx_name = "propertyKey", READ = property_key,
                    WRITE = set_property_key, NOTIFY = property_key_changed)]
        #[qproperty(QString, property_value, cxx_name = "propertyValue", READ = property_value,
                    WRITE = set_property_value, NOTIFY = property_value_changed)]
        #[qproperty(bool, reset, READ = reset, WRITE = set_reset, NOTIFY = reset_changed)]
        type WallpaperPropertySetQuery = super::WallpaperPropertySetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = params_changed)]
        #[qproperty(bool, clear, READ = clear, WRITE = set_clear, NOTIFY = params_changed)]
        #[qproperty(i32, fillmode, READ = fillmode, WRITE = set_fillmode,
                    NOTIFY = params_changed)]
        #[qproperty(i32, location_x, cxx_name = "locationX", READ = location_x,
                    WRITE = set_location_x, NOTIFY = params_changed)]
        #[qproperty(i32, location_y, cxx_name = "locationY", READ = location_y,
                    WRITE = set_location_y, NOTIFY = params_changed)]
        #[qproperty(i32, rotation, READ = rotation, WRITE = set_rotation,
                    NOTIFY = params_changed)]
        /// waywallen 0.4.4: отражение (`Flip`), панель деталей пишет его
        /// вместе с остальной раскладкой.
        #[qproperty(i32, flip, READ = flip, WRITE = set_flip, NOTIFY = params_changed)]
        type WallpaperLayoutSetQuery = super::WallpaperLayoutSetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        #[qproperty(QString, uri, READ = uri, NOTIFY = uri_changed)]
        type WallpaperApplyViaPortalQuery = super::WallpaperApplyViaPortalQueryRust;

        /// waywallen 0.4.4: скрыть обои из библиотеки или вернуть их.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId", READ = wallpaper_id,
                    WRITE = set_wallpaper_id, NOTIFY = wallpaper_id_changed)]
        #[qproperty(bool, hidden, READ = hidden, WRITE = set_hidden, NOTIFY = hidden_changed)]
        type WallpaperHideQuery = super::WallpaperHideQueryRust;
    }

    // ── Сигналы базы Query (одинаковые у всех семи) ───────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperApplyQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperApplyQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperApplyQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperApplyQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "wallpaperChanged"]
        fn wallpaper_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "targetsChanged"]
        fn targets_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "rendererNameChanged"]
        fn renderer_name_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "rendererIdChanged"]
        fn renderer_id_changed(self: Pin<&mut WallpaperApplyQuery>);
        #[qsignal]
        #[cxx_name = "stoppedPlaylistsChanged"]
        fn stopped_playlists_changed(self: Pin<&mut WallpaperApplyQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperGetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperGetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperGetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperGetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperGetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperGetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperGetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperGetQuery>);
        #[qsignal]
        #[cxx_name = "wallpaperChanged"]
        fn wallpaper_changed(self: Pin<&mut WallpaperGetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperRemoveQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperRemoveQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperRemoveQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperRemoveQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperRemoveQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperRemoveQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperRemoveQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperRemoveQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperRemoveQuery>);
        /// Удалён ОДИН предмет. Вендорный QML на него подписан
        /// (`WallpaperDetailPanel.qml:162`); у нашего Python он не эмитился
        /// никогда.
        #[qsignal]
        fn removed(self: Pin<&mut WallpaperRemoveQuery>, wallpaper_id: QString);
        /// Удалён набор: что просили и СКОЛЬКО демон снял на самом деле.
        #[qsignal]
        #[cxx_name = "removedMany"]
        fn removed_many(self: Pin<&mut WallpaperRemoveQuery>, wallpaper_ids: QStringList,
                        removed_count: i32);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperScanQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperScanQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperScanQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperScanQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperScanQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperScanQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperScanQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperScanQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut WallpaperScanQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperPropertySetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperPropertySetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperPropertySetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperPropertySetQuery>, token: i64, field: QString,
                        request: QByteArray);
        /// Сигналы по одному на свойство — как у оригинала. Наш Python
        /// схлопывал их в общий `paramsChanged`.
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "propertyKeyChanged"]
        fn property_key_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "propertyValueChanged"]
        fn property_value_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        /// Своё, протокола v0.2.5: у оригинала сброса свойства нет вовсе.
        #[qsignal]
        #[cxx_name = "resetChanged"]
        fn reset_changed(self: Pin<&mut WallpaperPropertySetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperLayoutSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperLayoutSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperLayoutSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        /// Один сигнал на все шесть свойств — так у оригинала (макрос WW_SET).
        #[qsignal]
        #[cxx_name = "paramsChanged"]
        fn params_changed(self: Pin<&mut WallpaperLayoutSetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperApplyViaPortalQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperApplyViaPortalQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperApplyViaPortalQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qsignal]
        #[cxx_name = "uriChanged"]
        fn uri_changed(self: Pin<&mut WallpaperApplyViaPortalQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WallpaperHideQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WallpaperHideQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WallpaperHideQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WallpaperHideQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WallpaperHideQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WallpaperHideQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WallpaperHideQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WallpaperHideQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperHideQuery>);
        #[qsignal]
        #[cxx_name = "hiddenChanged"]
        fn hidden_changed(self: Pin<&mut WallpaperHideQuery>);
        /// Что просили, в какую сторону и у скольких признак РЕАЛЬНО
        /// сменился (`updated_count` ответа демона) — тост страницы.
        #[qsignal]
        fn updated(self: Pin<&mut WallpaperHideQuery>, wallpaper_ids: QStringList,
                   hidden: bool, updated_count: u32);
    }

    // ── Геттеры и сеттеры ─────────────────────────────────────────────────
    extern "RustQt" {
        fn status(self: &WallpaperApplyQuery) -> i32;
        fn error(self: &WallpaperApplyQuery) -> QString;
        fn querying(self: &WallpaperApplyQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperApplyQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperApplyQuery>, value: bool);
        fn wallpaper(self: &WallpaperApplyQuery) -> QVariant;
        #[cxx_name = "setWallpaper"]
        fn set_wallpaper(self: Pin<&mut WallpaperApplyQuery>, value: QVariant);
        fn targets(self: &WallpaperApplyQuery) -> QVariant;
        #[cxx_name = "setTargets"]
        fn set_targets(self: Pin<&mut WallpaperApplyQuery>, value: QVariant);
        #[cxx_name = "rendererName"]
        fn renderer_name(self: &WallpaperApplyQuery) -> QString;
        #[cxx_name = "setRendererName"]
        fn set_renderer_name(self: Pin<&mut WallpaperApplyQuery>, value: QString);
        #[cxx_name = "rendererId"]
        fn renderer_id(self: &WallpaperApplyQuery) -> QString;
        #[cxx_name = "stoppedPlaylists"]
        fn stopped_playlists(self: &WallpaperApplyQuery) -> QVariant;

        fn status(self: &WallpaperGetQuery) -> i32;
        fn error(self: &WallpaperGetQuery) -> QString;
        fn querying(self: &WallpaperGetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperGetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperGetQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperGetQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperGetQuery>, value: QString);
        fn wallpaper(self: &WallpaperGetQuery) -> QVariant;

        fn status(self: &WallpaperRemoveQuery) -> i32;
        fn error(self: &WallpaperRemoveQuery) -> QString;
        fn querying(self: &WallpaperRemoveQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperRemoveQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperRemoveQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperRemoveQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperRemoveQuery>, value: QString);

        fn status(self: &WallpaperScanQuery) -> i32;
        fn error(self: &WallpaperScanQuery) -> QString;
        fn querying(self: &WallpaperScanQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperScanQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperScanQuery>, value: bool);
        fn count(self: &WallpaperScanQuery) -> i32;

        fn status(self: &WallpaperPropertySetQuery) -> i32;
        fn error(self: &WallpaperPropertySetQuery) -> QString;
        fn querying(self: &WallpaperPropertySetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperPropertySetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperPropertySetQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperPropertySetQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperPropertySetQuery>, value: QString);
        #[cxx_name = "propertyKey"]
        fn property_key(self: &WallpaperPropertySetQuery) -> QString;
        #[cxx_name = "setPropertyKey"]
        fn set_property_key(self: Pin<&mut WallpaperPropertySetQuery>, value: QString);
        #[cxx_name = "propertyValue"]
        fn property_value(self: &WallpaperPropertySetQuery) -> QString;
        #[cxx_name = "setPropertyValue"]
        fn set_property_value(self: Pin<&mut WallpaperPropertySetQuery>, value: QString);
        fn reset(self: &WallpaperPropertySetQuery) -> bool;
        #[cxx_name = "setReset"]
        fn set_reset(self: Pin<&mut WallpaperPropertySetQuery>, value: bool);

        fn status(self: &WallpaperLayoutSetQuery) -> i32;
        fn error(self: &WallpaperLayoutSetQuery) -> QString;
        fn querying(self: &WallpaperLayoutSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperLayoutSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperLayoutSetQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperLayoutSetQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperLayoutSetQuery>, value: QString);
        fn clear(self: &WallpaperLayoutSetQuery) -> bool;
        #[cxx_name = "setClear"]
        fn set_clear(self: Pin<&mut WallpaperLayoutSetQuery>, value: bool);
        fn fillmode(self: &WallpaperLayoutSetQuery) -> i32;
        #[cxx_name = "setFillmode"]
        fn set_fillmode(self: Pin<&mut WallpaperLayoutSetQuery>, value: i32);
        #[cxx_name = "locationX"]
        fn location_x(self: &WallpaperLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationX"]
        fn set_location_x(self: Pin<&mut WallpaperLayoutSetQuery>, value: i32);
        #[cxx_name = "locationY"]
        fn location_y(self: &WallpaperLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationY"]
        fn set_location_y(self: Pin<&mut WallpaperLayoutSetQuery>, value: i32);
        fn rotation(self: &WallpaperLayoutSetQuery) -> i32;
        #[cxx_name = "setRotation"]
        fn set_rotation(self: Pin<&mut WallpaperLayoutSetQuery>, value: i32);
        fn flip(self: &WallpaperLayoutSetQuery) -> i32;
        #[cxx_name = "setFlip"]
        fn set_flip(self: Pin<&mut WallpaperLayoutSetQuery>, value: i32);

        fn status(self: &WallpaperApplyViaPortalQuery) -> i32;
        fn error(self: &WallpaperApplyViaPortalQuery) -> QString;
        fn querying(self: &WallpaperApplyViaPortalQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperApplyViaPortalQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperApplyViaPortalQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperApplyViaPortalQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperApplyViaPortalQuery>, value: QString);
        fn uri(self: &WallpaperApplyViaPortalQuery) -> QString;

        fn status(self: &WallpaperHideQuery) -> i32;
        fn error(self: &WallpaperHideQuery) -> QString;
        fn querying(self: &WallpaperHideQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WallpaperHideQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WallpaperHideQuery>, value: bool);
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &WallpaperHideQuery) -> QString;
        #[cxx_name = "setWallpaperId"]
        fn set_wallpaper_id(self: Pin<&mut WallpaperHideQuery>, value: QString);
        fn hidden(self: &WallpaperHideQuery) -> bool;
        #[cxx_name = "setHidden"]
        fn set_hidden(self: Pin<&mut WallpaperHideQuery>, value: bool);
    }

    // ── Инвокаблы ─────────────────────────────────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperApplyQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperApplyQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperGetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperGetQuery>, token: i64, frame: &QByteArray);

        /// Удалить набор. Оригинал отсеивает пустые, при единственном
        /// идентификаторе кладёт его в `wallpaper_id` и шлёт `removed`.
        #[qinvokable]
        fn remove(self: Pin<&mut WallpaperRemoveQuery>, wallpaper_ids: QStringList);
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperRemoveQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperRemoveQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperScanQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperScanQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperPropertySetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperLayoutSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperLayoutSetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperApplyViaPortalQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperApplyViaPortalQuery>, token: i64,
                            frame: &QByteArray);

        /// `reload() == setHiddenForIds([wallpaperId], hidden)` оригинала.
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperHideQuery>);
        /// Скрыть (или вернуть) набор: пустые отсеиваются, единственный
        /// идентификатор уходит в `wallpaper_id`, как у оригинала.
        #[qinvokable]
        #[cxx_name = "setHiddenForIds"]
        fn set_hidden_for_ids(self: Pin<&mut WallpaperHideQuery>, wallpaper_ids: QStringList,
                              hidden: bool);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WallpaperHideQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Constructor<()> for WallpaperApplyQuery {}
    impl cxx_qt::Constructor<()> for WallpaperGetQuery {}
    impl cxx_qt::Constructor<()> for WallpaperRemoveQuery {}
    impl cxx_qt::Constructor<()> for WallpaperScanQuery {}
    impl cxx_qt::Constructor<()> for WallpaperPropertySetQuery {}
    impl cxx_qt::Constructor<()> for WallpaperLayoutSetQuery {}
    impl cxx_qt::Constructor<()> for WallpaperApplyViaPortalQuery {}
    impl cxx_qt::Constructor<()> for WallpaperHideQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QStringList, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_client;
use rexon_core::we_manage;
use rexon_core::we_query::{self, QuerySignal, QueryState, STATUS_ERROR};
use rexon_core::we_wallpaper;

use crate::qvalue::{json_to_variant, variant_to_json};
use crate::we_transport;

/// Дебаунс перезагрузки — 100 мс, как у оригинала (qextra query.cpp:16).
const DEBOUNCE_MS: i32 = 100;

/// Указатель на сам объект как на QObject.
fn self_ptr<T>(object: Pin<&mut T>) -> *mut qobject::QObject {
    let raw: *mut T = unsafe { object.get_unchecked_mut() };
    raw.cast()
}

/// Чем кончился запрос-действие.
///
/// Ветки разные не для красоты: у оригинала лямбду разбора зовёт
/// `inspect_set` ТОЛЬКО на успехе, а на отказе ставит Error и на этом
/// заканчивает (`qextra-ref/src/async.cppm:157`). Пока обе ветки шли одним
/// путём, после отказа демона выполнялся общий хвост `finish()` — статус
/// Error тут же перебивался на Finished, и страница видела успех там, где
/// демон отказал.
enum Answer {
    /// Демон ответил ошибкой: текст и статус Error уже посчитаны, разбирать
    /// нечего и доезжать до `finish()` нельзя.
    Failed(Vec<QuerySignal>),
    /// Демон ответил успехом; у части действий полезной нагрузки нет вовсе.
    Ok(Option<pb::response::Payload>),
}

/// Разобрать кадр ответа.
fn take_payload(st: &mut QueryState, frame: &[u8]) -> Result<Answer, prost::DecodeError> {
    let f = pb::ServerFrame::decode(frame)?;
    let Some(pb::server_frame::Kind::Response(rsp)) = f.kind else {
        return Err(prost::DecodeError::new("кадр сервера без ответа"));
    };
    if rsp.status != pb::Status::Ok as i32 {
        let text = we_query::response_error_text(rsp.status, &rsp.message);
        let mut out = we_query::set_error(st, &text);
        out.extend(we_query::set_status(st, STATUS_ERROR));
        return Ok(Answer::Failed(out));
    }
    Ok(Answer::Ok(rsp.payload))
}

/// Сигналы «кадр не разобрался»: демон может быть новее нас.
fn decode_failed(st: &mut QueryState) -> Vec<QuerySignal> {
    let mut out = we_query::set_error(st, &rexon_core::i18n::tr("could not parse the daemon response"));
    out.extend(we_query::set_status(st, STATUS_ERROR));
    out
}

/// Общая часть всех семи: свойства базы `Query`, отправка кадра и рассылка
/// сигналов базы. Это и есть обещанная планом замена Qt-наследования —
/// макрос печатает то, что у оригинала давал общий базовый класс.
macro_rules! impl_action_base {
    ($query:ident) => {
        impl qobject::$query {
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
                let signals =
                    we_query::set_forward_error(&mut self.as_mut().rust_mut().base, value);
                self.dispatch(signals);
            }

            /// Отправить кадр наружу общим швом партий 2-3.
            ///
            /// Возвращает токен или `None`, если соединения нет: тогда ядро
            /// уже перевело запрос в Error и слать нечего.
            pub fn send_frame(mut self: Pin<&mut Self>, field: &'static str, payload: Vec<u8>)
                -> Option<i64>
            {
                let connected = crate::discover_queries::connected();
                let (send, signals) =
                    we_query::begin_call(&mut self.as_mut().rust_mut().base, connected);
                self.as_mut().dispatch(signals);
                if !send {
                    return None;
                }
                // Номер выдаётся ДО сборки кадра — он же `request_id`
                // протокола.
                let token = crate::discover_queries::next_token();
                let Ok(bytes) = we_client::request_bytes(token as u64, field, &payload) else {
                    debug_assert!(false, "поле {field} неизвестно протоколу");
                    return None;
                };
                let owner = self_ptr(self.as_mut());
                // Владельца токена запоминаем ДО отправки: ответ может
                // прийти раньше, чем мы вернёмся из колбэка.
                unsafe { we_transport::remember(token, owner.cast()) };
                self.as_mut().send_request(
                    token,
                    QString::from(field),
                    QByteArray::from(&bytes[..]),
                );
                we_transport::send(token, field, &bytes);
                Some(token)
            }

            /// Разослать сигналы базы. Свойства читаются живые.
            pub fn dispatch_base(mut self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
                for signal in signals {
                    match signal {
                        QuerySignal::StatusChanged(v) => self.as_mut().status_changed(v),
                        QuerySignal::QueryingChanged => self.as_mut().querying_changed(),
                        QuerySignal::ErrorChanged => self.as_mut().error_changed(),
                        QuerySignal::ForwardErrorChanged => {
                            self.as_mut().forward_error_changed()
                        }
                        QuerySignal::Finished => self.as_mut().finished(),
                        QuerySignal::ErrorOccurred(text) => {
                            self.as_mut().error_occurred(QString::from(&text));
                        }
                        QuerySignal::AppErrorOccurred(text) => {
                            self.as_mut().app_error_occurred(QString::from(&text));
                        }
                        QuerySignal::Property(name) => self.as_mut().emit_property(name),
                    }
                }
            }
        }
    };
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperApplyQuery — применить обою к экранам
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperApplyQueryRust {
    base: QueryState,
    wallpaper: Value,
    /// Список как его присвоил QML — хранится дословно, разбирается в
    /// момент отправки (так же у оригинала: `m_targets` — QVariantList,
    /// а `presentationTargetsFromVariant` зовётся в `reload`).
    targets: Value,
    renderer_name: String,
    renderer_id: String,
    /// Плейлисты, остановленные ПОСЛЕДНИМ применением
    /// (`we_wallpaper::stopped_playlists`); всегда массив.
    stopped_playlists: Value,
    pending: std::collections::BTreeSet<i64>,
}

impl_action_base!(WallpaperApplyQuery);

impl cxx_qt::Initialize for qobject::WallpaperApplyQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        rust.wallpaper = Value::Null;
        rust.stopped_playlists = Value::Array(Vec::new());
    }
}

impl qobject::WallpaperApplyQuery {
    pub fn wallpaper(&self) -> QVariant {
        json_to_variant(&self.wallpaper)
    }

    /// Сравнение ТОЛЬКО по `id_proto` — как у оригинала. Наш Python
    /// присваивал и слал сигнал всегда, даже когда пришла та же обоя.
    pub fn set_wallpaper(mut self: Pin<&mut Self>, value: QVariant) {
        let next = variant_to_json(&value);
        let id_of = |v: &Value| {
            v.get("id_proto")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        if id_of(&self.wallpaper) == id_of(&next) {
            return;
        }
        self.as_mut().rust_mut().wallpaper = next;
        self.wallpaper_changed();
    }

    pub fn targets(&self) -> QVariant {
        json_to_variant(&self.targets)
    }

    /// Сравнение списка целиком — как у оригинала (`setTargets`).
    pub fn set_targets(mut self: Pin<&mut Self>, value: QVariant) {
        let next = match variant_to_json(&value) {
            arr @ Value::Array(_) => arr,
            _ => Value::Array(Vec::new()),
        };
        if self.targets == next {
            return;
        }
        self.as_mut().rust_mut().targets = next;
        self.targets_changed();
    }

    pub fn renderer_name(&self) -> QString {
        QString::from(&self.renderer_name)
    }

    pub fn set_renderer_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.renderer_name == next {
            return;
        }
        self.as_mut().rust_mut().renderer_name = next;
        self.renderer_name_changed();
    }

    pub fn renderer_id(&self) -> QString {
        QString::from(&self.renderer_id)
    }

    pub fn stopped_playlists(&self) -> QVariant {
        json_to_variant(&self.stopped_playlists)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let wallpaper_id = self
            .wallpaper
            .get("id_proto")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if wallpaper_id.is_empty() {
            return;
        }
        // Список прошлого применения к новому не относится: чистим ДО
        // отправки, как у оригинала, — иначе при отказе демона панель
        // прочла бы плейлисты предыдущего ответа.
        if self.stopped_playlists.as_array().is_some_and(|a| !a.is_empty()) {
            self.as_mut().rust_mut().stopped_playlists = Value::Array(Vec::new());
            self.as_mut().stopped_playlists_changed();
        }
        // Вход ровно один — `targets`, как у оригинала: в
        // `WallpaperApplyRequest` v0.3.5 другого поля адресации нет.
        // Пустой список — законное значение, а не «ничего не задано»:
        // демон трактует его как «каждый холст по разу плюс каждый
        // самостоятельный дисплей».
        let targets = we_manage::presentation_targets(&self.targets);
        let payload = pb::WallpaperApplyRequest {
            wallpaper_id,
            targets,
            renderer_name: self.renderer_name.clone(),
            ..Default::default()
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_apply", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let payload = match parsed {
            Ok(Answer::Ok(payload)) => payload,
            Ok(Answer::Failed(signals)) => {
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        let mut signals = Vec::new();
        if let Some(pb::response::Payload::WallpaperApply(wa)) = payload {
            // ⚠️ Ровно то, что терял Python: имя вставшего рендерера. Сигнал
            // БЕЗУСЛОВНЫЙ, как у оригинала (wallpaper_query.cpp:539): он
            // означает «демон ответил», а не «значение стало другим».
            let mut rust = self.as_mut().rust_mut();
            rust.stopped_playlists = we_wallpaper::stopped_playlists(&wa);
            rust.renderer_id = wa.renderer_id;
            signals.push(QuerySignal::Property("rendererId"));
            // Тоже безусловный: панель читает список по переходу статуса,
            // и оба свойства обязаны быть свежими ДО него.
            signals.push(QuerySignal::Property("stoppedPlaylists"));
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_query::finish(&mut rust.base, signals)
        };
        self.dispatch(signals);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "rendererId" => self.as_mut().renderer_id_changed(),
            "stoppedPlaylists" => self.as_mut().stopped_playlists_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperGetQuery — свежий вид одной обои для панели деталей
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperGetQueryRust {
    base: QueryState,
    data: we_wallpaper::WallpaperGetData,
    wallpaper_id: String,
    /// token → какой предмет им спрашивали. Ответ на устаревший запрос
    /// отбрасывается — иначе панель показывает чужую обоину.
    pending: std::collections::BTreeMap<i64, String>,
    debounce: *mut qobject::QObject,
}

impl_action_base!(WallpaperGetQuery);

impl cxx_qt::Initialize for qobject::WallpaperGetQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner = self_ptr(self.as_mut());
        let timer = unsafe { qobject::rexon_new_action_debounce(owner, DEBOUNCE_MS) };
        self.as_mut().rust_mut().debounce = timer;
    }
}

impl qobject::WallpaperGetQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.as_mut().wallpaper_id_changed();
        // connect_requet_reload оригинала: смена предмета → отложенная
        // перезагрузка.
        let timer = self.debounce;
        if !timer.is_null() {
            unsafe { qobject::rexon_restart_action_debounce(timer) };
        }
    }

    pub fn wallpaper(&self) -> QVariant {
        json_to_variant(&Value::Object(self.data.wallpaper.clone()))
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.wallpaper_id.is_empty() {
            return;
        }
        let wanted = self.wallpaper_id.clone();
        let payload = pb::WallpaperGetRequest {
            wallpaper_id: wanted.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_get", payload) {
            self.rust_mut().pending.insert(token, wanted);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(wanted) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        // ⚠️ Защита от устаревшего ответа (`wallpaper_query.cpp:311`):
        // пока ответ шёл, панель могла переключиться на другую карточку.
        if wanted != self.wallpaper_id {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_wallpaper::apply_wallpaper_get(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "wallpaper" => self.as_mut().wallpaper_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperRemoveQuery — удалить обои из библиотеки
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperRemoveQueryRust {
    base: QueryState,
    wallpaper_id: String,
    /// token → что именно просили удалить: `removedMany` обязан вернуть
    /// в QML тот же набор.
    pending: std::collections::BTreeMap<i64, Vec<String>>,
}

impl_action_base!(WallpaperRemoveQuery);

impl cxx_qt::Initialize for qobject::WallpaperRemoveQuery {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

impl qobject::WallpaperRemoveQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.wallpaper_id_changed();
    }

    pub fn remove(mut self: Pin<&mut Self>, wallpaper_ids: QStringList) {
        // Пустые идентификаторы отсеиваются — Python слал их демону как есть.
        let ids: Vec<String> = wallpaper_ids
            .iter()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty() {
            return;
        }
        // Единственный предмет уезжает в `wallpaper_id`, а не списком —
        // так у оригинала.
        let payload = if ids.len() == 1 {
            pb::WallpaperRemoveRequest {
                wallpaper_id: ids[0].clone(),
                ..Default::default()
            }
        } else {
            pb::WallpaperRemoveRequest {
                wallpaper_ids: ids.clone(),
                ..Default::default()
            }
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_remove", payload) {
            self.rust_mut().pending.insert(token, ids);
        }
    }

    /// `reload() == remove([wallpaperId])` оригинала. У нашего Python
    /// переопределения не было вовсе, и свойство было мёртвым.
    pub fn reload(mut self: Pin<&mut Self>) {
        let id = self.wallpaper_id.clone();
        let mut list = QStringList::default();
        list.append(QString::from(&id));
        self.as_mut().remove(list);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(ids) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let payload = match parsed {
            Ok(Answer::Ok(payload)) => payload,
            Ok(Answer::Failed(signals)) => {
                // Демон отказал — отчёта об удалении нет вовсе.
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        // ⚠️ Число берётся из ОТВЕТА демона, а не из длины отправленного
        // набора: Python обещал «удалено N», сколько бы демон ни снял.
        let removed_count = match payload {
            Some(pb::response::Payload::WallpaperRemove(wr)) => wr.removed_count as i32,
            _ => 0,
        };
        // ⚠️ Отчёт уходит ДО перевода в Finished: у оригинала он эмитится
        // внутри лямбды разбора, а статус ставится уже после неё. Пока было
        // наоборот, обработчик QML на `finished` срабатывал раньше, чем
        // страница узнавала, что и сколько удалено.
        if ids.len() == 1 {
            self.as_mut().removed(QString::from(&ids[0]));
        }
        let list = QStringList::from_iter(ids.iter().map(QString::from));
        self.as_mut().removed_many(list, removed_count);
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_query::finish(&mut rust.base, Vec::new())
        };
        self.dispatch(signals);
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        let _ = self;
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperScanQuery — пересканировать библиотеку
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperScanQueryRust {
    base: QueryState,
    count: i32,
    pending: std::collections::BTreeSet<i64>,
}

impl_action_base!(WallpaperScanQuery);

impl cxx_qt::Initialize for qobject::WallpaperScanQuery {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

impl qobject::WallpaperScanQuery {
    pub fn count(&self) -> i32 {
        self.count
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::WallpaperScanRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_scan", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let payload = match parsed {
            Ok(Answer::Ok(payload)) => payload,
            Ok(Answer::Failed(signals)) => {
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        let mut signals = Vec::new();
        if let Some(pb::response::Payload::WallpaperScan(ws)) = payload {
            // Сигнал БЕЗУСЛОВНЫЙ (wallpaper_query.cpp:270): пересканирование
            // с тем же числом обоев — всё равно событие «демон досчитал».
            let mut rust = self.as_mut().rust_mut();
            rust.count = ws.count as i32;
            signals.push(QuerySignal::Property("count"));
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_query::finish(&mut rust.base, signals)
        };
        self.dispatch(signals);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "count" => self.as_mut().count_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperPropertySetQuery — записать одно свойство обои
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperPropertySetQueryRust {
    base: QueryState,
    wallpaper_id: String,
    property_key: String,
    property_value: String,
    reset: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl_action_base!(WallpaperPropertySetQuery);

impl cxx_qt::Initialize for qobject::WallpaperPropertySetQuery {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

impl qobject::WallpaperPropertySetQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.wallpaper_id_changed();
    }

    pub fn property_key(&self) -> QString {
        QString::from(&self.property_key)
    }

    pub fn set_property_key(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.property_key == next {
            return;
        }
        self.as_mut().rust_mut().property_key = next;
        self.property_key_changed();
    }

    pub fn property_value(&self) -> QString {
        QString::from(&self.property_value)
    }

    pub fn set_property_value(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.property_value == next {
            return;
        }
        self.as_mut().rust_mut().property_value = next;
        self.property_value_changed();
    }

    pub fn reset(&self) -> bool {
        self.reset
    }

    pub fn set_reset(mut self: Pin<&mut Self>, value: bool) {
        if self.reset == value {
            return;
        }
        self.as_mut().rust_mut().reset = value;
        self.reset_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.wallpaper_id.is_empty() || self.property_key.is_empty() {
            return;
        }
        // v0.2.5: пустое `value` — валидный оверрайд (текстовые свойства),
        // сброс к значению из project.json — ТОЛЬКО `reset = true`.
        let operation = if self.reset {
            pb::wallpaper_property_set_request::Operation::Reset(true)
        } else {
            pb::wallpaper_property_set_request::Operation::Value(self.property_value.clone())
        };
        let payload = pb::WallpaperPropertySetRequest {
            wallpaper_id: self.wallpaper_id.clone(),
            key: self.property_key.clone(),
            operation: Some(operation),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("wallpaper_property_set", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        // Полезной нагрузки у ответа нет: успех — это отсутствие ошибки.
        let signals = match parsed {
            Ok(Answer::Ok(_)) => {
                let mut rust = self.as_mut().rust_mut();
                we_query::finish(&mut rust.base, Vec::new())
            }
            Ok(Answer::Failed(signals)) => signals,
            Err(_) => decode_failed(&mut self.as_mut().rust_mut().base),
        };
        self.dispatch(signals);
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        let _ = self;
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperLayoutSetQuery — персональная раскладка одной обои
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperLayoutSetQueryRust {
    base: QueryState,
    data: we_wallpaper::LayoutSetData,
    pending: std::collections::BTreeSet<i64>,
}

impl_action_base!(WallpaperLayoutSetQuery);

impl cxx_qt::Initialize for qobject::WallpaperLayoutSetQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        // waywallen 0.4.4: `m_flip { 1 }` оригинала — FLIP_NONE, а не
        // «не задано»: раскладка обоины всегда несёт явное отражение.
        self.as_mut().rust_mut().data.flip = pb::Flip::None as i32;
    }
}

impl qobject::WallpaperLayoutSetQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.data.wallpaper_id)
    }

    pub fn set_wallpaper_id(self: Pin<&mut Self>, value: QString) {
        self.set_layout("wallpaperId", Value::String(value.to_string()));
    }

    pub fn clear(&self) -> bool {
        self.data.clear
    }

    pub fn set_clear(self: Pin<&mut Self>, value: bool) {
        self.set_layout("clear", Value::Bool(value));
    }

    pub fn fillmode(&self) -> i32 {
        self.data.fillmode
    }

    pub fn set_fillmode(self: Pin<&mut Self>, value: i32) {
        self.set_layout("fillmode", Value::from(value));
    }

    pub fn location_x(&self) -> i32 {
        self.data.location_x
    }

    pub fn set_location_x(self: Pin<&mut Self>, value: i32) {
        self.set_layout("locationX", Value::from(value));
    }

    pub fn location_y(&self) -> i32 {
        self.data.location_y
    }

    pub fn set_location_y(self: Pin<&mut Self>, value: i32) {
        self.set_layout("locationY", Value::from(value));
    }

    pub fn rotation(&self) -> i32 {
        self.data.rotation
    }

    pub fn set_rotation(self: Pin<&mut Self>, value: i32) {
        self.set_layout("rotation", Value::from(value));
    }

    pub fn flip(&self) -> i32 {
        self.data.flip
    }

    pub fn set_flip(self: Pin<&mut Self>, value: i32) {
        self.set_layout("flip", Value::from(value));
    }

    /// Сравнение и сигнал считает ядро — у оригинала это макрос WW_SET.
    pub fn set_layout(mut self: Pin<&mut Self>, name: &str, value: Value) {
        let Some(signals) = ({
            let mut rust = self.as_mut().rust_mut();
            we_wallpaper::set_layout_property(&mut rust.data, name, &value)
        }) else {
            debug_assert!(false, "нет свойства {name} у WallpaperLayoutSetQuery");
            return;
        };
        self.dispatch(signals);
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        // `None` — обои не выбрана: оригинал выходит ДО перевода статуса,
        // то есть запрос не начинается вовсе.
        let Some(payload) = we_wallpaper::encode_layout_set(&self.data) else {
            return;
        };
        if let Some(token) = self.as_mut().send_frame("wallpaper_layout_set", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let signals = match parsed {
            Ok(Answer::Ok(_)) => {
                let mut rust = self.as_mut().rust_mut();
                we_query::finish(&mut rust.base, Vec::new())
            }
            Ok(Answer::Failed(signals)) => signals,
            Err(_) => decode_failed(&mut self.as_mut().rust_mut().base),
        };
        self.dispatch(signals);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "params" => self.as_mut().params_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperApplyViaPortalQuery — отдать обою через портал рабочего стола
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperApplyViaPortalQueryRust {
    base: QueryState,
    wallpaper_id: String,
    uri: String,
    pending: std::collections::BTreeSet<i64>,
}

impl_action_base!(WallpaperApplyViaPortalQuery);

impl cxx_qt::Initialize for qobject::WallpaperApplyViaPortalQuery {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

impl qobject::WallpaperApplyViaPortalQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.wallpaper_id_changed();
    }

    pub fn uri(&self) -> QString {
        QString::from(&self.uri)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.wallpaper_id.is_empty() {
            return;
        }
        let payload = pb::WallpaperApplyViaPortalRequest {
            wallpaper_id: self.wallpaper_id.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self
            .as_mut()
            .send_frame("wallpaper_apply_via_portal", payload)
        {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let payload = match parsed {
            Ok(Answer::Ok(payload)) => payload,
            Ok(Answer::Failed(signals)) => {
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        let mut signals = Vec::new();
        if let Some(pb::response::Payload::WallpaperApplyViaPortal(wp)) = payload {
            // Сигнал безусловный — wallpaper_query.cpp:574.
            let mut rust = self.as_mut().rust_mut();
            rust.uri = wp.uri;
            signals.push(QuerySignal::Property("uri"));
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_query::finish(&mut rust.base, signals)
        };
        self.dispatch(signals);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "uri" => self.as_mut().uri_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кадр с целью-дисплеем обязан совпасть с тем, который живой демон
    /// 0.3.5 принял в замере 18.08 (обоя 71 встала на дисплей 2). Кадр с
    /// упакованными `display_ids` (`0a0237312a0102`) тот же демон роняет
    /// молча — потому цели и собираются по схеме v0.3.5.
    #[test]
    fn target_frame_matches_live_daemon() {
        let targets = we_manage::presentation_targets(&serde_json::json!([{"displayId": 2}]));
        let bytes = pb::WallpaperApplyRequest {
            wallpaper_id: "71".into(),
            targets,
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(hex(&bytes), "0a0237312a020802");
    }

    /// Номера дисплеев, пропущенные через ядровый `display_targets`, дают
    /// тот же кадр, что и карты целей из QML: движок сборки целей один на
    /// всю вкладку (правило #12), второй формы адресации нет.
    #[test]
    fn display_numbers_give_the_same_frame() {
        let bytes = pb::WallpaperApplyRequest {
            wallpaper_id: "71".into(),
            targets: we_client::display_targets(&[2]),
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(hex(&bytes), "0a0237312a020802");
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ════════════════════════════════════════════════════════════════════════
// WallpaperHideQuery — скрыть обои из библиотеки (waywallen 0.4.4)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperHideQueryRust {
    base: QueryState,
    wallpaper_id: String,
    hidden: bool,
    /// token → (что просили, в какую сторону): `updated` обязан вернуть в
    /// QML ровно то, что уходило, даже если свойства с тех пор сменились.
    pending: std::collections::BTreeMap<i64, (Vec<String>, bool)>,
}

impl_action_base!(WallpaperHideQuery);

impl cxx_qt::Initialize for qobject::WallpaperHideQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        // `m_hidden { true }` оригинала: голый `reload()` прячет.
        self.as_mut().rust_mut().hidden = true;
    }
}

impl qobject::WallpaperHideQuery {
    pub fn wallpaper_id(&self) -> QString {
        QString::from(&self.wallpaper_id)
    }

    pub fn set_wallpaper_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wallpaper_id == next {
            return;
        }
        self.as_mut().rust_mut().wallpaper_id = next;
        self.wallpaper_id_changed();
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn set_hidden(mut self: Pin<&mut Self>, value: bool) {
        if self.hidden == value {
            return;
        }
        self.as_mut().rust_mut().hidden = value;
        self.hidden_changed();
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let mut list = QStringList::default();
        list.append(QString::from(&self.wallpaper_id));
        let hidden = self.hidden;
        self.as_mut().set_hidden_for_ids(list, hidden);
    }

    pub fn set_hidden_for_ids(mut self: Pin<&mut Self>, wallpaper_ids: QStringList, hidden: bool) {
        let ids: Vec<String> = wallpaper_ids.iter().map(|s| s.to_string()).collect();
        let Some(payload) = we_wallpaper::encode_hide_request(&ids, hidden) else {
            return;
        };
        let ids = we_wallpaper::non_empty_ids(&ids);
        if let Some(token) = self.as_mut().send_frame("wallpaper_hide", payload) {
            self.rust_mut().pending.insert(token, (ids, hidden));
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some((ids, hidden)) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            take_payload(&mut rust.base, frame.as_slice())
        };
        let payload = match parsed {
            Ok(Answer::Ok(payload)) => payload,
            Ok(Answer::Failed(signals)) => {
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        let updated_count = match payload {
            Some(pb::response::Payload::WallpaperHide(wh)) => wh.updated_count,
            _ => 0,
        };
        // Отчёт ДО перевода в Finished — как у `WallpaperRemoveQuery`: у
        // оригинала `updated` эмитится внутри лямбды разбора.
        let list = QStringList::from_iter(ids.iter().map(QString::from));
        self.as_mut().updated(list, hidden, updated_count);
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            we_query::finish(&mut rust.base, Vec::new())
        };
        self.dispatch(signals);
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        let _ = self;
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
