// The "Displays" page: canvases and the displays themselves — layout,
// canvas editing and a display alias.
//
// Canvases appeared at the vendor in v0.3.5; neither the C++ layer nor the
// Python bridge knew them — because of that `DisplaysPage.qml` did not
// compile at all ("W.CanvasLayoutSetQuery is not a type"), and the tab lost
// the whole page rather than one button. Batch 8 added the neighbours from
// the vendor file here — `DisplayLayoutSetQuery` and `DisplayRenameQuery`.
//
// Reference: the vendor's `ui/src/query/display_query.cpp` at tag v0.3.5
// (f7baecd). Response parsing is in the core (`rexon_core::we_manage`);
// here there is only the Qt side.
//
// What the port fixes along the way (rule #16):
//
//   1. The position in `display_layout_set` was not clamped to 0..100 — the
//      original has `std::clamp`, the Python bridge had nothing. A value out
//      of range went to the daemon as is.
//   2. The core wrapper `we_client::display_rename` did not send
//      `display_id` at all, although since v0.2.4 the daemon resolves the
//      target precisely by it and keeps `name` for old clients. Fixed in
//      the wrapper itself — it is shared.
//
// waywallen 0.4.2 added `DisplayPauseSetQuery` — manually pausing one
// display (a button on the "Displays" page). Reference: `display_query.cpp`
// of tag v0.4.2 (4d45025).

/// cxx-qt bridge: canvases and displays of the "Displays" page.
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
        /// waywallen 0.4.4: flip — whether it is set, which one, and reset.
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
        /// Warning: `f64`, not `u64`: we do not expose a 64-bit integer to QML — a
        /// literal binding "displayId: 7" fails together with the whole document.
        /// The analysis and precision limits are in `src/qvalue.rs`.
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
        /// waywallen 0.4.4: flip — whether it is set, which one, and reset.
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
        /// Warning: `f64`, not `u64`: we do not expose a 64-bit integer to QML — a
        /// literal binding "displayId: 7" fails together with the whole document.
        /// The analysis and precision limits are in `src/qvalue.rs`.
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
        /// Warning: `f64`, not `u64` — for the same reason as the neighbours
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
        /// Warning: `revision` goes out as `f64` for the same reason as the
        /// properties: a signal parameter with the `unsigned long` metatype is
        /// unknown to the QML engine. The vendor's handler is `onCanvasUpdated:
        /// function (canvasId, revision)` (DisplaysPage.qml:185).
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
// CanvasLayoutSetQuery — canvas layout (fill, position, rotation)
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
    /// waywallen 0.4.4: flip (the original's `m_flip { 1 }` — FLIP_NONE).
    flip_set: bool,
    flip: i32,
    clear_flip: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl Default for CanvasLayoutSetQueryRust {
    /// The default position is the centre (50/50), as in the original. The
    /// rest are zeros of the derived `Default`.
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

/// A setter that the original's `WW_SET` macro emits: when the value
/// changes, ONE shared `paramsChanged` fires.
/// The same for a 64-bit field: exposed to QML as `f64`, inside an exact
/// `u64`. A separate macro rather than a parameter of the previous one,
/// because the facade type and the field type are DIFFERENT here (why — see
/// `src/qvalue.rs`).
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
        // Percentages are clamped to 0..100 right here, as in the original: the
        // page also clamps them with its clampPercent, but the frame must be valid
        // when called from elsewhere too.
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
                // waywallen 0.4.4: flip — the "Flip" control of the "Displays" page
                // (`FlipButtons`).
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
        // The original parses the answer with an empty lambda: only the outcome
        // matters.
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
// CanvasMutationQuery — create, rewrite, delete a canvas
// ════════════════════════════════════════════════════════════════════════

/// A frame prepared by a call from QML and not yet sent.
struct PreparedRequest {
    field: &'static str,
    payload: Vec<u8>,
    /// Number of the canvas being deleted — the daemon does not repeat it in
    /// the answer.
    deleted_id: String,
}

#[derive(Default)]
pub struct CanvasMutationQueryRust {
    base: QueryState,
    request: Option<PreparedRequest>,
    /// Token → number of the canvas being deleted (empty if the frame is not
    /// about deletion).
    pending: std::collections::BTreeMap<i64, String>,
}

impl_query_base!(CanvasMutationQuery);

impl qobject::CanvasMutationQuery {
    /// There is no prepared frame — return silently. The original behaves the
    /// same way: `reload()` without `m_request` does nothing, and this matters
    /// because the base calls `reload()` on its own debounce.
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
// DisplayLayoutSetQuery — layout of one display (DisplaysPage.qml)
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
    /// waywallen 0.4.4: flip (the original's `m_flip { 1 }` — FLIP_NONE).
    flip_set: bool,
    flip: i32,
    clear_flip: bool,
    pending: std::collections::BTreeSet<i64>,
}

impl Default for DisplayLayoutSetQueryRust {
    /// Zeros, as the derived `Default` gave, except for the flip: in the
    /// original it defaults to FLIP_NONE (v0.4.4).
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
        // Percentages are clamped to 0..100, as in the original (`std::clamp` in
        // display_query.cpp). The Python bridge did not do this: a value out of
        // range went to the daemon as is (rule #16).
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
                // waywallen 0.4.4: flip — the "Flip" control of the "Displays" page
                // (`FlipButtons`).
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
        // The daemon broadcasts the write with the `display_changed` event — the
        // live DisplayManager picks it up by itself; here there is only the
        // outcome.
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
// DisplayRenameQuery — display alias (DisplaysPage.qml)
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
// DisplayPauseSetQuery — manual pause of one display (waywallen 0.4.2)
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
        // As in the original, only the outcome of the answer matters: the daemon
        // broadcasts the new pause state with the `display_changed` event, and the
        // live DisplayManager picks it up (`manualPaused`/`effectivePaused`).
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
