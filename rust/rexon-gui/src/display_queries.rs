// Страница «Дисплеи»: холсты (canvas) и сами дисплеи — раскладка,
// правка холстов и псевдоним дисплея.
//
// Холсты
// появились у вендора в v0.3.5, ни C++-слой, ни Python-мост их не знали —
// из-за этого `DisplaysPage.qml` не компилировалась вовсе
// («W.CanvasLayoutSetQuery is not a type»), и вкладка теряла страницу
// целиком, а не одну кнопку. Партия 8 добавила сюда соседей по вендорному
// файлу — `DisplayLayoutSetQuery` и `DisplayRenameQuery`.
//
// Эталон — `ui/src/query/display_query.cpp` вендора на теге v0.3.5
// (f7baecd). Разбор ответа — в ядре (`rexon_core::we_manage`), здесь
// только Qt-сторона.
//
// Что порт чинит по дороге (правило #16):
//
//   1. Положение в `display_layout_set` не резалось в 0..100 — у
//      оригинала `std::clamp`, у питоновского моста ничего. Значение мимо
//      диапазона уезжало демону как есть.
//   2. Обёртка ядра `we_client::display_rename` не слала `display_id`
//      вовсе, хотя с v0.2.4 демон резолвит цель именно по нему, а `name`
//      держит для старых клиентов. Починено в самой обёртке — она общая.
//
// waywallen 0.4.2 добавил `DisplayPauseSetQuery` — ручную паузу одного
// дисплея (кнопка на странице «Дисплеи»). Эталон — `display_query.cpp`
// тега v0.4.2 (4d45025).

/// Мост cxx-qt: холсты и дисплеи страницы «Дисплеи».
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, canvas_id, cxx_name = "canvasId", READ = canvas_id,
                    WRITE = set_canvas_id, NOTIFY = params_changed)]
        #[qproperty(bool, fillmode_set, cxx_name = "fillmodeSet", READ = fillmode_set,
                    WRITE = set_fillmode_set, NOTIFY = params_changed)]
        #[qproperty(i32, fillmode, READ = fillmode, WRITE = set_fillmode,
                    NOTIFY = params_changed)]
        #[qproperty(bool, location_set, cxx_name = "locationSet", READ = location_set,
                    WRITE = set_location_set, NOTIFY = params_changed)]
        #[qproperty(i32, location_x, cxx_name = "locationX", READ = location_x,
                    WRITE = set_location_x, NOTIFY = params_changed)]
        #[qproperty(i32, location_y, cxx_name = "locationY", READ = location_y,
                    WRITE = set_location_y, NOTIFY = params_changed)]
        #[qproperty(bool, rotation_set, cxx_name = "rotationSet", READ = rotation_set,
                    WRITE = set_rotation_set, NOTIFY = params_changed)]
        #[qproperty(i32, rotation, READ = rotation, WRITE = set_rotation,
                    NOTIFY = params_changed)]
        #[qproperty(bool, clear_fillmode, cxx_name = "clearFillmode", READ = clear_fillmode,
                    WRITE = set_clear_fillmode, NOTIFY = params_changed)]
        #[qproperty(bool, clear_location, cxx_name = "clearLocation", READ = clear_location,
                    WRITE = set_clear_location, NOTIFY = params_changed)]
        #[qproperty(bool, clear_rotation, cxx_name = "clearRotation", READ = clear_rotation,
                    WRITE = set_clear_rotation, NOTIFY = params_changed)]
        /// waywallen 0.4.4: отражение — задано ли, какое, и сброс.
        #[qproperty(bool, flip_set, cxx_name = "flipSet", READ = flip_set,
                    WRITE = set_flip_set, NOTIFY = params_changed)]
        #[qproperty(i32, flip, READ = flip, WRITE = set_flip, NOTIFY = params_changed)]
        #[qproperty(bool, clear_flip, cxx_name = "clearFlip", READ = clear_flip,
                    WRITE = set_clear_flip, NOTIFY = params_changed)]
        type CanvasLayoutSetQuery = super::CanvasLayoutSetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        type CanvasMutationQuery = super::CanvasMutationQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, name, READ = name, WRITE = set_name, NOTIFY = params_changed)]
        /// ⚠️ `f64`, а не `u64`: 64-битное целое наружу в QML не выпускаем
        /// — литеральная привязка «displayId: 7» отказывает вместе со всем
        /// документом. Разбор и границы точности — `src/qvalue.rs`.
        #[qproperty(f64, display_id, cxx_name = "displayId", READ = display_id,
                    WRITE = set_display_id, NOTIFY = params_changed)]
        #[qproperty(bool, fillmode_set, cxx_name = "fillmodeSet", READ = fillmode_set,
                    WRITE = set_fillmode_set, NOTIFY = params_changed)]
        #[qproperty(i32, fillmode, READ = fillmode, WRITE = set_fillmode,
                    NOTIFY = params_changed)]
        #[qproperty(bool, location_set, cxx_name = "locationSet", READ = location_set,
                    WRITE = set_location_set, NOTIFY = params_changed)]
        #[qproperty(i32, location_x, cxx_name = "locationX", READ = location_x,
                    WRITE = set_location_x, NOTIFY = params_changed)]
        #[qproperty(i32, location_y, cxx_name = "locationY", READ = location_y,
                    WRITE = set_location_y, NOTIFY = params_changed)]
        #[qproperty(bool, align_set, cxx_name = "alignSet", READ = align_set,
                    WRITE = set_align_set, NOTIFY = params_changed)]
        #[qproperty(i32, align, READ = align, WRITE = set_align, NOTIFY = params_changed)]
        #[qproperty(bool, rotation_set, cxx_name = "rotationSet", READ = rotation_set,
                    WRITE = set_rotation_set, NOTIFY = params_changed)]
        #[qproperty(i32, rotation, READ = rotation, WRITE = set_rotation,
                    NOTIFY = params_changed)]
        #[qproperty(bool, clear_fillmode, cxx_name = "clearFillmode", READ = clear_fillmode,
                    WRITE = set_clear_fillmode, NOTIFY = params_changed)]
        #[qproperty(bool, clear_location, cxx_name = "clearLocation", READ = clear_location,
                    WRITE = set_clear_location, NOTIFY = params_changed)]
        #[qproperty(bool, clear_align, cxx_name = "clearAlign", READ = clear_align,
                    WRITE = set_clear_align, NOTIFY = params_changed)]
        #[qproperty(bool, clear_rotation, cxx_name = "clearRotation", READ = clear_rotation,
                    WRITE = set_clear_rotation, NOTIFY = params_changed)]
        /// waywallen 0.4.4: отражение — задано ли, какое, и сброс.
        #[qproperty(bool, flip_set, cxx_name = "flipSet", READ = flip_set,
                    WRITE = set_flip_set, NOTIFY = params_changed)]
        #[qproperty(i32, flip, READ = flip, WRITE = set_flip, NOTIFY = params_changed)]
        #[qproperty(bool, clear_flip, cxx_name = "clearFlip", READ = clear_flip,
                    WRITE = set_clear_flip, NOTIFY = params_changed)]
        type DisplayLayoutSetQuery = super::DisplayLayoutSetQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, name, READ = name, WRITE = set_name, NOTIFY = params_changed)]
        /// ⚠️ `f64`, а не `u64`: 64-битное целое наружу в QML не выпускаем
        /// — литеральная привязка «displayId: 7» отказывает вместе со всем
        /// документом. Разбор и границы точности — `src/qvalue.rs`.
        #[qproperty(f64, display_id, cxx_name = "displayId", READ = display_id,
                    WRITE = set_display_id, NOTIFY = params_changed)]
        #[qproperty(QString, alias, READ = alias, WRITE = set_alias, NOTIFY = params_changed)]
        #[qproperty(bool, clear, READ = clear, WRITE = set_clear, NOTIFY = params_changed)]
        type DisplayRenameQuery = super::DisplayRenameQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// ⚠️ `f64`, а не `u64` — по той же причине, что у соседей
        /// (`src/qvalue.rs`).
        #[qproperty(f64, display_id, cxx_name = "displayId", READ = display_id,
                    WRITE = set_display_id, NOTIFY = params_changed)]
        #[qproperty(bool, paused, READ = paused, WRITE = set_paused, NOTIFY = params_changed)]
        type DisplayPauseSetQuery = super::DisplayPauseSetQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut CanvasLayoutSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut CanvasLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut CanvasLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut CanvasLayoutSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut CanvasLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut CanvasLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut CanvasLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut CanvasLayoutSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "paramsChanged"]
        fn params_changed(self: Pin<&mut CanvasLayoutSetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut CanvasMutationQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut CanvasMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut CanvasMutationQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut CanvasMutationQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut CanvasMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut CanvasMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut CanvasMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut CanvasMutationQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "canvasCreated"]
        fn canvas_created(self: Pin<&mut CanvasMutationQuery>, canvas_id: QString);
        #[qsignal]
        #[cxx_name = "canvasUpdated"]
        /// ⚠️ `revision` уходит `f64` по той же причине, что и свойства:
        /// параметр сигнала с метатипом `unsigned long` движку QML
        /// незнаком. Обработчик у вендора — `onCanvasUpdated: function
        /// (canvasId, revision)` (DisplaysPage.qml:185).
        fn canvas_updated(self: Pin<&mut CanvasMutationQuery>, canvas_id: QString,
                          revision: f64);
        #[qsignal]
        #[cxx_name = "canvasDeleted"]
        fn canvas_deleted(self: Pin<&mut CanvasMutationQuery>, canvas_id: QString);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DisplayLayoutSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut DisplayLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut DisplayLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut DisplayLayoutSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut DisplayLayoutSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut DisplayLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut DisplayLayoutSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut DisplayLayoutSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "paramsChanged"]
        fn params_changed(self: Pin<&mut DisplayLayoutSetQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DisplayRenameQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut DisplayRenameQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut DisplayRenameQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut DisplayRenameQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut DisplayRenameQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut DisplayRenameQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut DisplayRenameQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut DisplayRenameQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "paramsChanged"]
        fn params_changed(self: Pin<&mut DisplayRenameQuery>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DisplayPauseSetQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut DisplayPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut DisplayPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut DisplayPauseSetQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut DisplayPauseSetQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut DisplayPauseSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut DisplayPauseSetQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut DisplayPauseSetQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "paramsChanged"]
        fn params_changed(self: Pin<&mut DisplayPauseSetQuery>);
    }

    extern "RustQt" {
        fn status(self: &CanvasLayoutSetQuery) -> i32;
        fn error(self: &CanvasLayoutSetQuery) -> QString;
        fn querying(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);

        fn status(self: &CanvasMutationQuery) -> i32;
        fn error(self: &CanvasMutationQuery) -> QString;
        fn querying(self: &CanvasMutationQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &CanvasMutationQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut CanvasMutationQuery>, value: bool);

        #[cxx_name = "canvasId"]
        fn canvas_id(self: &CanvasLayoutSetQuery) -> QString;
        #[cxx_name = "setCanvasId"]
        fn set_canvas_id(self: Pin<&mut CanvasLayoutSetQuery>, value: QString);
        #[cxx_name = "fillmodeSet"]
        fn fillmode_set(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setFillmodeSet"]
        fn set_fillmode_set(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        fn fillmode(self: &CanvasLayoutSetQuery) -> i32;
        #[cxx_name = "setFillmode"]
        fn set_fillmode(self: Pin<&mut CanvasLayoutSetQuery>, value: i32);
        #[cxx_name = "locationSet"]
        fn location_set(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setLocationSet"]
        fn set_location_set(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        #[cxx_name = "locationX"]
        fn location_x(self: &CanvasLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationX"]
        fn set_location_x(self: Pin<&mut CanvasLayoutSetQuery>, value: i32);
        #[cxx_name = "locationY"]
        fn location_y(self: &CanvasLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationY"]
        fn set_location_y(self: Pin<&mut CanvasLayoutSetQuery>, value: i32);
        #[cxx_name = "rotationSet"]
        fn rotation_set(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setRotationSet"]
        fn set_rotation_set(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        fn rotation(self: &CanvasLayoutSetQuery) -> i32;
        #[cxx_name = "setRotation"]
        fn set_rotation(self: Pin<&mut CanvasLayoutSetQuery>, value: i32);
        #[cxx_name = "clearFillmode"]
        fn clear_fillmode(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setClearFillmode"]
        fn set_clear_fillmode(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        #[cxx_name = "clearLocation"]
        fn clear_location(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setClearLocation"]
        fn set_clear_location(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        #[cxx_name = "clearRotation"]
        fn clear_rotation(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setClearRotation"]
        fn set_clear_rotation(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        #[cxx_name = "flipSet"]
        fn flip_set(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setFlipSet"]
        fn set_flip_set(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);
        fn flip(self: &CanvasLayoutSetQuery) -> i32;
        #[cxx_name = "setFlip"]
        fn set_flip(self: Pin<&mut CanvasLayoutSetQuery>, value: i32);
        #[cxx_name = "clearFlip"]
        fn clear_flip(self: &CanvasLayoutSetQuery) -> bool;
        #[cxx_name = "setClearFlip"]
        fn set_clear_flip(self: Pin<&mut CanvasLayoutSetQuery>, value: bool);

        fn status(self: &DisplayLayoutSetQuery) -> i32;
        fn error(self: &DisplayLayoutSetQuery) -> QString;
        fn querying(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        fn name(self: &DisplayLayoutSetQuery) -> QString;
        #[cxx_name = "setName"]
        fn set_name(self: Pin<&mut DisplayLayoutSetQuery>, value: QString);
        #[cxx_name = "displayId"]
        fn display_id(self: &DisplayLayoutSetQuery) -> f64;
        #[cxx_name = "setDisplayId"]
        fn set_display_id(self: Pin<&mut DisplayLayoutSetQuery>, value: f64);
        #[cxx_name = "fillmodeSet"]
        fn fillmode_set(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setFillmodeSet"]
        fn set_fillmode_set(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        fn fillmode(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setFillmode"]
        fn set_fillmode(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "locationSet"]
        fn location_set(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setLocationSet"]
        fn set_location_set(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        #[cxx_name = "locationX"]
        fn location_x(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationX"]
        fn set_location_x(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "locationY"]
        fn location_y(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setLocationY"]
        fn set_location_y(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "alignSet"]
        fn align_set(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setAlignSet"]
        fn set_align_set(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        fn align(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setAlign"]
        fn set_align(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "rotationSet"]
        fn rotation_set(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setRotationSet"]
        fn set_rotation_set(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        fn rotation(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setRotation"]
        fn set_rotation(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "clearFillmode"]
        fn clear_fillmode(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setClearFillmode"]
        fn set_clear_fillmode(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        #[cxx_name = "clearLocation"]
        fn clear_location(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setClearLocation"]
        fn set_clear_location(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        #[cxx_name = "clearAlign"]
        fn clear_align(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setClearAlign"]
        fn set_clear_align(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        #[cxx_name = "clearRotation"]
        fn clear_rotation(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setClearRotation"]
        fn set_clear_rotation(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        #[cxx_name = "flipSet"]
        fn flip_set(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setFlipSet"]
        fn set_flip_set(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);
        fn flip(self: &DisplayLayoutSetQuery) -> i32;
        #[cxx_name = "setFlip"]
        fn set_flip(self: Pin<&mut DisplayLayoutSetQuery>, value: i32);
        #[cxx_name = "clearFlip"]
        fn clear_flip(self: &DisplayLayoutSetQuery) -> bool;
        #[cxx_name = "setClearFlip"]
        fn set_clear_flip(self: Pin<&mut DisplayLayoutSetQuery>, value: bool);

        fn status(self: &DisplayRenameQuery) -> i32;
        fn error(self: &DisplayRenameQuery) -> QString;
        fn querying(self: &DisplayRenameQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &DisplayRenameQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut DisplayRenameQuery>, value: bool);
        fn name(self: &DisplayRenameQuery) -> QString;
        #[cxx_name = "setName"]
        fn set_name(self: Pin<&mut DisplayRenameQuery>, value: QString);
        #[cxx_name = "displayId"]
        fn display_id(self: &DisplayRenameQuery) -> f64;
        #[cxx_name = "setDisplayId"]
        fn set_display_id(self: Pin<&mut DisplayRenameQuery>, value: f64);
        fn alias(self: &DisplayRenameQuery) -> QString;
        #[cxx_name = "setAlias"]
        fn set_alias(self: Pin<&mut DisplayRenameQuery>, value: QString);
        fn clear(self: &DisplayRenameQuery) -> bool;
        #[cxx_name = "setClear"]
        fn set_clear(self: Pin<&mut DisplayRenameQuery>, value: bool);

        fn status(self: &DisplayPauseSetQuery) -> i32;
        fn error(self: &DisplayPauseSetQuery) -> QString;
        fn querying(self: &DisplayPauseSetQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &DisplayPauseSetQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut DisplayPauseSetQuery>, value: bool);
        #[cxx_name = "displayId"]
        fn display_id(self: &DisplayPauseSetQuery) -> f64;
        #[cxx_name = "setDisplayId"]
        fn set_display_id(self: Pin<&mut DisplayPauseSetQuery>, value: f64);
        fn paused(self: &DisplayPauseSetQuery) -> bool;
        #[cxx_name = "setPaused"]
        fn set_paused(self: Pin<&mut DisplayPauseSetQuery>, value: bool);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut CanvasLayoutSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut CanvasLayoutSetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut CanvasMutationQuery>);
        #[qinvokable]
        #[cxx_name = "createCanvas"]
        fn create_canvas(self: Pin<&mut CanvasMutationQuery>, name: QString,
                         members: QVariant);
        #[qinvokable]
        #[cxx_name = "updateCanvas"]
        fn update_canvas(self: Pin<&mut CanvasMutationQuery>, canvas_id: QString,
                         expected_revision: f64, name: QString, members: QVariant);
        #[qinvokable]
        #[cxx_name = "removeCanvas"]
        fn remove_canvas(self: Pin<&mut CanvasMutationQuery>, canvas_id: QString,
                         expected_revision: f64);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut CanvasMutationQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut DisplayLayoutSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut DisplayLayoutSetQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut DisplayRenameQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut DisplayRenameQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut DisplayPauseSetQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut DisplayPauseSetQuery>, token: i64,
                            frame: &QByteArray);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QVariant};
use prost::Message;

use rexon_core::proto::control as pb;
use rexon_core::we_manage::{apply_canvas_mutation, canvas_members_from_json, CanvasMutation};
use rexon_core::we_query::{apply_ack, QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::variant_to_json;

// ════════════════════════════════════════════════════════════════════════
// CanvasLayoutSetQuery — раскладка холста (заполнение, положение, поворот)
// ════════════════════════════════════════════════════════════════════════

pub struct CanvasLayoutSetQueryRust {
    base: QueryState,
    canvas_id: String,
    fillmode_set: bool,
    fillmode: i32,
    location_set: bool,
    location_x: i32,
    location_y: i32,
    rotation_set: bool,
    rotation: i32,
    clear_fillmode: bool,
    clear_location: bool,
    clear_rotation: bool,
    /// waywallen 0.4.4: отражение (`m_flip { 1 }` оригинала — FLIP_NONE).
    flip_set: bool,
    flip: i32,
    clear_flip: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl Default for CanvasLayoutSetQueryRust {
    /// Положение по умолчанию — центр (50/50), как у оригинала. Остальное
    /// — нули производного `Default`.
    fn default() -> Self {
        CanvasLayoutSetQueryRust {
            base: QueryState::default(),
            canvas_id: String::new(),
            fillmode_set: false,
            fillmode: 0,
            location_set: false,
            location_x: 50,
            location_y: 50,
            rotation_set: false,
            rotation: 0,
            clear_fillmode: false,
            clear_location: false,
            clear_rotation: false,
            flip_set: false,
            flip: pb::Flip::None as i32,
            clear_flip: false,
            pending: std::collections::BTreeSet::new(),
        }
    }
}

impl_query_base!(CanvasLayoutSetQuery);

/// Сеттер, который у оригинала печатает макрос `WW_SET`: значение
/// меняется — летит ОДИН общий `paramsChanged`.
/// То же для 64-битного поля: наружу в QML — `f64`, внутри — точное `u64`.
/// Отдельный макрос, а не параметр к прежнему, потому что тип фасада и тип
/// поля здесь РАЗНЫЕ (почему — `src/qvalue.rs`).
macro_rules! param_setter_u64 {
    ($setter:ident, $field:ident) => {
        pub fn $setter(mut self: Pin<&mut Self>, value: f64) {
            let value = crate::qvalue::qml_to_u64(value);
            if self.$field == value {
                return;
            }
            self.as_mut().rust_mut().$field = value;
            self.params_changed();
        }
    };
}

macro_rules! param_setter {
    ($setter:ident, $field:ident, $ty:ty) => {
        pub fn $setter(mut self: Pin<&mut Self>, value: $ty) {
            if self.$field == value {
                return;
            }
            self.as_mut().rust_mut().$field = value;
            self.params_changed();
        }
    };
}

impl qobject::CanvasLayoutSetQuery {
    pub fn canvas_id(&self) -> QString {
        QString::from(&self.canvas_id)
    }

    pub fn set_canvas_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.canvas_id == next {
            return;
        }
        self.as_mut().rust_mut().canvas_id = next;
        self.params_changed();
    }

    pub fn fillmode_set(&self) -> bool {
        self.fillmode_set
    }
    pub fn fillmode(&self) -> i32 {
        self.fillmode
    }
    pub fn location_set(&self) -> bool {
        self.location_set
    }
    pub fn location_x(&self) -> i32 {
        self.location_x
    }
    pub fn location_y(&self) -> i32 {
        self.location_y
    }
    pub fn rotation_set(&self) -> bool {
        self.rotation_set
    }
    pub fn rotation(&self) -> i32 {
        self.rotation
    }
    pub fn clear_fillmode(&self) -> bool {
        self.clear_fillmode
    }
    pub fn clear_location(&self) -> bool {
        self.clear_location
    }
    pub fn clear_rotation(&self) -> bool {
        self.clear_rotation
    }

    param_setter!(set_fillmode_set, fillmode_set, bool);
    param_setter!(set_fillmode, fillmode, i32);
    param_setter!(set_location_set, location_set, bool);
    param_setter!(set_location_x, location_x, i32);
    param_setter!(set_location_y, location_y, i32);
    param_setter!(set_rotation_set, rotation_set, bool);
    param_setter!(set_rotation, rotation, i32);
    param_setter!(set_clear_fillmode, clear_fillmode, bool);
    param_setter!(set_clear_location, clear_location, bool);
    param_setter!(set_clear_rotation, clear_rotation, bool);
    param_setter!(set_flip_set, flip_set, bool);
    param_setter!(set_flip, flip, i32);
    param_setter!(set_clear_flip, clear_flip, bool);

    pub fn flip_set(&self) -> bool {
        self.flip_set
    }
    pub fn flip(&self) -> i32 {
        self.flip
    }
    pub fn clear_flip(&self) -> bool {
        self.clear_flip
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        // Проценты режутся в 0..100 здесь же, как у оригинала: страница
        // тоже режет их своим clampPercent, но кадр обязан быть валидным
        // и при вызове из другого места.
        let payload = pb::CanvasLayoutSetRequest {
            canvas_id: self.canvas_id.clone(),
            r#override: Some(pb::LayoutOverride {
                fillmode_set: self.fillmode_set,
                fillmode: self.fillmode,
                align_set: false,
                align: 0,
                rotation_set: self.rotation_set,
                rotation: self.rotation,
                location_set: self.location_set,
                location_x: self.location_x.clamp(0, 100) as u32,
                location_y: self.location_y.clamp(0, 100) as u32,
                // waywallen 0.4.4: отражение — орган «Отражение» страницы
                // «Дисплеи» (`FlipButtons`).
                flip_set: self.flip_set,
                flip: self.flip,
            }),
            clear_fillmode: self.clear_fillmode,
            clear_rotation: self.clear_rotation,
            clear_location: self.clear_location,
            clear_flip: self.clear_flip,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("canvas_layout_set", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // Оригинал разбирает ответ пустой лямбдой: важен только исход.
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// CanvasMutationQuery — создать, переписать, удалить холст
// ════════════════════════════════════════════════════════════════════════

/// Кадр, подготовленный вызовом из QML и ещё не ушедший.
struct PreparedRequest {
    field: &'static str,
    payload: Vec<u8>,
    /// Номер удаляемого холста — демон в ответе его не повторяет.
    deleted_id: String,
}

#[derive(Default)]
pub struct CanvasMutationQueryRust {
    base: QueryState,
    request: Option<PreparedRequest>,
    /// Токен → номер удаляемого холста (пусто, если кадр не про удаление).
    pending: std::collections::BTreeMap<i64, String>,
}

impl_query_base!(CanvasMutationQuery);

impl qobject::CanvasMutationQuery {
    /// Подготовленного кадра нет — молча выйти. Так же ведёт себя
    /// оригинал: `reload()` без `m_request` не делает ничего, и это важно,
    /// потому что база зовёт `reload()` по своему дебаунсу.
    pub fn reload(mut self: Pin<&mut Self>) {
        let Some(request) = self.as_mut().rust_mut().request.take() else {
            return;
        };
        if let Some(token) = self.as_mut().send_frame(request.field, request.payload) {
            self.rust_mut().pending.insert(token, request.deleted_id);
        }
    }

    pub fn create_canvas(mut self: Pin<&mut Self>, name: QString, members: QVariant) {
        let payload = pb::CanvasCreateRequest {
            name: name.to_string(),
            members: canvas_members_from_json(&variant_to_json(&members)),
            layout_override: None,
        }
        .encode_to_vec();
        self.as_mut().rust_mut().request = Some(PreparedRequest {
            field: "canvas_create",
            payload,
            deleted_id: String::new(),
        });
        self.reload();
    }

    pub fn update_canvas(
        mut self: Pin<&mut Self>,
        canvas_id: QString,
        expected_revision: f64,
        name: QString,
        members: QVariant,
    ) {
        let payload = pb::CanvasUpdateRequest {
            canvas_id: canvas_id.to_string(),
            expected_revision: crate::qvalue::qml_to_u64(expected_revision),
            name: name.to_string(),
            members: canvas_members_from_json(&variant_to_json(&members)),
        }
        .encode_to_vec();
        self.as_mut().rust_mut().request = Some(PreparedRequest {
            field: "canvas_update",
            payload,
            deleted_id: String::new(),
        });
        self.reload();
    }

    pub fn remove_canvas(
        mut self: Pin<&mut Self>,
        canvas_id: QString,
        expected_revision: f64,
    ) {
        let expected_revision = crate::qvalue::qml_to_u64(expected_revision);
        let id = canvas_id.to_string();
        let payload = pb::CanvasDeleteRequest {
            canvas_id: id.clone(),
            expected_revision,
        }
        .encode_to_vec();
        self.as_mut().rust_mut().request = Some(PreparedRequest {
            field: "canvas_delete",
            payload,
            deleted_id: id,
        });
        self.reload();
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(deleted_id) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            apply_canvas_mutation(&mut rust.base, &deleted_id, frame.as_slice())
        };
        match parsed {
            Ok((signals, outcome)) => {
                self.as_mut().dispatch(signals);
                match outcome {
                    CanvasMutation::Created(id) => {
                        self.canvas_created(QString::from(&id));
                    }
                    CanvasMutation::Updated(id, revision) => {
                        self.canvas_updated(
                            QString::from(&id),
                            crate::qvalue::u64_to_qml(revision),
                        );
                    }
                    CanvasMutation::Deleted(id) => {
                        self.canvas_deleted(QString::from(&id));
                    }
                    CanvasMutation::None => {}
                }
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// DisplayLayoutSetQuery — раскладка одного дисплея (DisplaysPage.qml)
// ════════════════════════════════════════════════════════════════════════

pub struct DisplayLayoutSetQueryRust {
    base: QueryState,
    name: String,
    display_id: u64,
    fillmode_set: bool,
    fillmode: i32,
    location_set: bool,
    location_x: i32,
    location_y: i32,
    align_set: bool,
    align: i32,
    rotation_set: bool,
    rotation: i32,
    clear_fillmode: bool,
    clear_location: bool,
    clear_align: bool,
    clear_rotation: bool,
    /// waywallen 0.4.4: отражение (`m_flip { 1 }` оригинала — FLIP_NONE).
    flip_set: bool,
    flip: i32,
    clear_flip: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl Default for DisplayLayoutSetQueryRust {
    /// Нули, как давал производный `Default`, кроме отражения: у оригинала
    /// оно по умолчанию FLIP_NONE (v0.4.4).
    fn default() -> Self {
        DisplayLayoutSetQueryRust {
            base: QueryState::default(),
            name: String::new(),
            display_id: 0,
            fillmode_set: false,
            fillmode: 0,
            location_set: false,
            location_x: 0,
            location_y: 0,
            align_set: false,
            align: 0,
            rotation_set: false,
            rotation: 0,
            clear_fillmode: false,
            clear_location: false,
            clear_align: false,
            clear_rotation: false,
            flip_set: false,
            flip: pb::Flip::None as i32,
            clear_flip: false,
            pending: std::collections::BTreeSet::new(),
        }
    }
}

impl_query_base!(DisplayLayoutSetQuery);

impl qobject::DisplayLayoutSetQuery {
    pub fn name(&self) -> QString {
        QString::from(&self.name)
    }

    pub fn set_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.name == next {
            return;
        }
        self.as_mut().rust_mut().name = next;
        self.params_changed();
    }

    pub fn display_id(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.display_id)
    }
    pub fn fillmode_set(&self) -> bool {
        self.fillmode_set
    }
    pub fn fillmode(&self) -> i32 {
        self.fillmode
    }
    pub fn location_set(&self) -> bool {
        self.location_set
    }
    pub fn location_x(&self) -> i32 {
        self.location_x
    }
    pub fn location_y(&self) -> i32 {
        self.location_y
    }
    pub fn align_set(&self) -> bool {
        self.align_set
    }
    pub fn align(&self) -> i32 {
        self.align
    }
    pub fn rotation_set(&self) -> bool {
        self.rotation_set
    }
    pub fn rotation(&self) -> i32 {
        self.rotation
    }
    pub fn clear_fillmode(&self) -> bool {
        self.clear_fillmode
    }
    pub fn clear_location(&self) -> bool {
        self.clear_location
    }
    pub fn clear_align(&self) -> bool {
        self.clear_align
    }
    pub fn clear_rotation(&self) -> bool {
        self.clear_rotation
    }

    param_setter_u64!(set_display_id, display_id);
    param_setter!(set_fillmode_set, fillmode_set, bool);
    param_setter!(set_fillmode, fillmode, i32);
    param_setter!(set_location_set, location_set, bool);
    param_setter!(set_location_x, location_x, i32);
    param_setter!(set_location_y, location_y, i32);
    param_setter!(set_align_set, align_set, bool);
    param_setter!(set_align, align, i32);
    param_setter!(set_rotation_set, rotation_set, bool);
    param_setter!(set_rotation, rotation, i32);
    param_setter!(set_clear_fillmode, clear_fillmode, bool);
    param_setter!(set_clear_location, clear_location, bool);
    param_setter!(set_clear_align, clear_align, bool);
    param_setter!(set_clear_rotation, clear_rotation, bool);
    param_setter!(set_flip_set, flip_set, bool);
    param_setter!(set_flip, flip, i32);
    param_setter!(set_clear_flip, clear_flip, bool);

    pub fn flip_set(&self) -> bool {
        self.flip_set
    }
    pub fn flip(&self) -> i32 {
        self.flip
    }
    pub fn clear_flip(&self) -> bool {
        self.clear_flip
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        // Проценты режутся в 0..100, как у оригинала (`std::clamp` в
        // display_query.cpp). Питоновский мост этого не делал: значение
        // мимо диапазона уезжало демону как есть (правило #16).
        let payload = pb::DisplayLayoutSetRequest {
            name: self.name.clone(),
            display_id: self.display_id,
            r#override: Some(pb::LayoutOverride {
                fillmode_set: self.fillmode_set,
                fillmode: self.fillmode,
                align_set: self.align_set,
                align: self.align,
                rotation_set: self.rotation_set,
                rotation: self.rotation,
                location_set: self.location_set,
                location_x: self.location_x.clamp(0, 100) as u32,
                location_y: self.location_y.clamp(0, 100) as u32,
                // waywallen 0.4.4: отражение — орган «Отражение» страницы
                // «Дисплеи» (`FlipButtons`).
                flip_set: self.flip_set,
                flip: self.flip,
            }),
            clear_fillmode: self.clear_fillmode,
            clear_align: self.clear_align,
            clear_rotation: self.clear_rotation,
            clear_location: self.clear_location,
            clear_flip: self.clear_flip,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("display_layout_set", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // Запись демон разносит событием `display_changed` — живой
        // DisplayManager подхватит её сам, здесь только исход.
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// DisplayRenameQuery — псевдоним дисплея (DisplaysPage.qml)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct DisplayRenameQueryRust {
    base: QueryState,
    name: String,
    display_id: u64,
    alias: String,
    clear: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(DisplayRenameQuery);

impl qobject::DisplayRenameQuery {
    pub fn name(&self) -> QString {
        QString::from(&self.name)
    }

    pub fn set_name(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.name == next {
            return;
        }
        self.as_mut().rust_mut().name = next;
        self.params_changed();
    }

    pub fn alias(&self) -> QString {
        QString::from(&self.alias)
    }

    pub fn set_alias(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.alias == next {
            return;
        }
        self.as_mut().rust_mut().alias = next;
        self.params_changed();
    }

    pub fn display_id(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.display_id)
    }
    pub fn clear(&self) -> bool {
        self.clear
    }

    param_setter_u64!(set_display_id, display_id);
    param_setter!(set_clear, clear, bool);

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::DisplayRenameRequest {
            name: self.name.clone(),
            display_id: self.display_id,
            alias: self.alias.clone(),
            clear: self.clear,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("display_rename", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// DisplayPauseSetQuery — ручная пауза одного дисплея (waywallen 0.4.2)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct DisplayPauseSetQueryRust {
    base: QueryState,
    display_id: u64,
    paused: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(DisplayPauseSetQuery);

impl qobject::DisplayPauseSetQuery {
    pub fn display_id(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.display_id)
    }
    pub fn paused(&self) -> bool {
        self.paused
    }

    param_setter_u64!(set_display_id, display_id);
    param_setter!(set_paused, paused, bool);

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::DisplayPauseSetRequest {
            display_id: self.display_id,
            paused: self.paused,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("display_pause_set", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        // Как у оригинала, из ответа важен только исход: новое состояние
        // паузы демон разносит событием `display_changed`, и его подхватит
        // живой DisplayManager (`manualPaused`/`effectivePaused`).
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            apply_ack(&mut rust.base, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(self: Pin<&mut Self>, name: &str) {
        debug_assert!(false, "нет сигнала для свойства {name}");
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
