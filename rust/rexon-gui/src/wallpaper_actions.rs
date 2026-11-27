// Action queries of the "Wallpapers" page: apply, read, remove, rescan,
// write a property, set the layout, hand over through the portal.
//
// The wallpaper list and the selection live next door
// (wallpaper_queries.rs, wallpaper_list_model.rs,
// wallpaper_select_storage.rs).
//
// Warning: the reference is the ORIGINAL (waywallen
// `ui/src/query/wallpaper_query.cpp`), not our Python. The port fixes five
// divergences that porting "as it was" would have preserved (rule #16):
//
//   1. `WallpaperApplyQuery` THREW AWAY the daemon's answer
//      (`lambda rsp: None`), while `renderer_id` arrives in it. So our
//      `rendererId` property was forever empty, and `rendererIdChanged` was
//      never sent.
//   2. `WallpaperRemoveQuery.remove()` did not filter out empty identifiers,
//      always sent a list and NEVER emitted `removed(id)` — while the
//      vendored QML subscribes to it (`WallpaperDetailPanel.qml:162`), that
//      is, the handler was dead. With a single id the original puts it into
//      `wallpaper_id`, sends `removed` and only then `removedMany`.
//   3. `removedMany` carried the length of the SENT set instead of
//      `removed_count` from the daemon's ANSWER: the toast promised "3
//      removed" when the daemon removed one.
//   4. `WallpaperRemoveQuery.reload()` was not overridden at all — the
//      `wallpaperId` property of this query was dead. In the original
//      `reload() == remove([wallpaperId])`.
//   5. `WallpaperGetQuery` did not check whether the right item arrived. The
//      original compares `m_wallpaper_id` with the requested one and silently
//      returns on a mismatch — otherwise when switching cards quickly the
//      details panel shows SOMEONE ELSE's wallpaper.
//
// Each of the five fixes is covered by its own golden replay case
// (`wallpaper_page_golden.rs`, the `--wallpaper-selftest` bench): removing
// any of them immediately fails the comparison — verified with a bite.
//
// A deliberate difference from the original, NOT a defect: for
// `WallpaperPropertySetQuery` the v0.2.5 protocol gives
// `oneof operation { value | reset }`, which the original does not have at
// all. Resetting a property is our branch, and it stays. The property
// signals, however, are split one per property, as in the original: our
// Python collapsed them all into a shared `paramsChanged`.

// waywallen 0.4.4 added an eighth one — `WallpaperHideQuery` (hide
// wallpapers from the library and restore them), and the `flip` of
// `WallpaperLayoutSetQuery`.

/// cxx-qt bridge: action queries of the "Wallpapers" page.
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
        /// Deferred reload timer under its own name — a third declaration of the
        /// same C++ function in the crate would give a duplicate symbol.
        ///
        /// # Safety
        /// `owner` is a live QObject with a `reload()` invokable.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_action_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` is a timer created by `rexon_new_action_debounce`.
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
        /// Apply targets of v0.3.5: a list of maps, each with EITHER `displayId` OR
        /// `canvasId`. Empty = "all selectable targets" (the daemon treats it the
        /// same way). The property is mandatory: `WallpaperDetailPanel.qml:343`
        /// assigns it in the Apply button handler, and assigning a non-existent
        /// property in QML is an exception after which the handler aborts and the
        /// request does not go out at all.
        #[qproperty(QVariant, targets, READ = targets, WRITE = set_targets,
                    NOTIFY = targets_changed)]
        #[qproperty(QString, renderer_name, cxx_name = "rendererName", READ = renderer_name,
                    WRITE = set_renderer_name, NOTIFY = renderer_name_changed)]
        #[qproperty(QString, renderer_id, cxx_name = "rendererId", READ = renderer_id,
                    NOTIFY = renderer_id_changed)]
        /// Playlists the daemon stopped with this apply: a list of maps
        /// `{playlistId, playlistName, allDisplays, displayIds}`. The details panel
        /// shows a toast for each (`WallpaperDetailPanel.qml`,
        /// `toastStoppedPlaylists`).
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
        /// waywallen 0.4.4: flip (`Flip`); the details panel writes it together with
        /// the rest of the layout.
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

        /// waywallen 0.4.4: hide wallpapers from the library or restore them.
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

    // ── Signals of the Query base (the same for all seven) ────────────────
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
        /// ONE item was removed. The vendored QML subscribes to it
        /// (`WallpaperDetailPanel.qml:162`); our Python never emitted it.
        #[qsignal]
        fn removed(self: Pin<&mut WallpaperRemoveQuery>, wallpaper_id: QString);
        /// A set was removed: what was asked for and HOW MANY the daemon actually
        /// removed.
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
        /// Signals one per property — as in the original. Our Python collapsed them
        /// into a shared `paramsChanged`.
        #[qsignal]
        #[cxx_name = "wallpaperIdChanged"]
        fn wallpaper_id_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "propertyKeyChanged"]
        fn property_key_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        #[qsignal]
        #[cxx_name = "propertyValueChanged"]
        fn property_value_changed(self: Pin<&mut WallpaperPropertySetQuery>);
        /// Our own, protocol v0.2.5: the original has no property reset at all.
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
        /// One signal for all six properties — as in the original (the WW_SET
        /// macro).
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
        /// What was asked for, in which direction and for how many the flag REALLY
        /// changed (`updated_count` of the daemon's answer) — the page's toast.
        #[qsignal]
        fn updated(self: Pin<&mut WallpaperHideQuery>, wallpaper_ids: QStringList,
                   hidden: bool, updated_count: u32);
    }

    // ── Getters and setters ───────────────────────────────────────────────
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

    // ── Invokables ────────────────────────────────────────────────────────
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

        /// Remove a set. The original filters out empty ones, with a single
        /// identifier puts it into `wallpaper_id` and sends `removed`.
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

        /// The original's `reload() == setHiddenForIds([wallpaperId], hidden)`.
        #[qinvokable]
        fn reload(self: Pin<&mut WallpaperHideQuery>);
        /// Hide (or restore) a set: empty ones are filtered out, a single
        /// identifier goes into `wallpaper_id`, as in the original.
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

/// Reload debounce — 100 ms, as in the original (qextra query.cpp:16).
const DEBOUNCE_MS: i32 = 100;

/// Pointer to the object itself as a QObject.
fn self_ptr<T>(object: Pin<&mut T>) -> *mut qobject::QObject {
    let raw: *mut T = unsafe { object.get_unchecked_mut() };
    raw.cast()
}

/// How the action query ended.
///
/// The branches differ not for looks: in the original the parsing lambda is
/// called by `inspect_set` ONLY on success, while on a refusal it sets Error
/// and stops there (`qextra-ref/src/async.cppm:157`). While both branches
/// went the same way, after the daemon's refusal the shared `finish()` tail
/// ran — the Error status was immediately overwritten with Finished, and
/// the page saw success where the daemon had refused.
enum Answer {
    /// The daemon answered with an error: the text and the Error status are
    /// already computed, there is nothing to parse, and `finish()` must not be
    /// reached.
    Failed(Vec<QuerySignal>),
    /// The daemon answered with success; some actions have no payload at all.
    Ok(Option<pb::response::Payload>),
}

/// Parse the answer frame.
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

/// "The frame did not parse" signals: the daemon may be newer than us.
fn decode_failed(st: &mut QueryState) -> Vec<QuerySignal> {
    let mut out = we_query::set_error(st, &rexon_core::i18n::tr("could not parse the daemon response"));
    out.extend(we_query::set_status(st, STATUS_ERROR));
    out
}

/// The part shared by all seven: properties of the `Query` base, sending a
/// frame and emitting the base's signals. This is the replacement for Qt
/// inheritance promised by the plan — the macro emits what the original got
/// from a shared base class.
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

            /// Send the frame out through the shared seam of batches 2-3.
            ///
            /// Returns a token, or `None` if there is no connection: then the core has
            /// already switched the request to Error and there is nothing to send.
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
                // The number is issued BEFORE the frame is built — it is also the
                // protocol's `request_id`.
                let token = crate::discover_queries::next_token();
                let Ok(bytes) = we_client::request_bytes(token as u64, field, &payload) else {
                    debug_assert!(false, "поле {field} неизвестно протоколу");
                    return None;
                };
                let owner = self_ptr(self.as_mut());
                // The token owner is remembered BEFORE sending: the answer may arrive
                // before we return from the callback.
                unsafe { we_transport::remember(token, owner.cast()) };
                self.as_mut().send_request(
                    token,
                    QString::from(field),
                    QByteArray::from(&bytes[..]),
                );
                we_transport::send(token, field, &bytes);
                Some(token)
            }

            /// Emit the base's signals. The properties are read live.
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
// WallpaperApplyQuery — apply a wallpaper to screens
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperApplyQueryRust {
    base: QueryState,
    wallpaper: Value,
    /// The list as QML assigned it — stored verbatim and parsed at sending time
    /// (the same in the original: `m_targets` is a QVariantList, and
    /// `presentationTargetsFromVariant` is called in `reload`).
    targets: Value,
    renderer_name: String,
    renderer_id: String,
    /// Playlists stopped by the LAST apply (`we_wallpaper::stopped_playlists`);
    /// always an array.
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

    /// Comparison ONLY by `id_proto` — as in the original. Our Python assigned
    /// and sent the signal always, even when the same wallpaper arrived.
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

    /// Comparing the whole list — as in the original (`setTargets`).
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
        // The list of the previous apply has nothing to do with the new one: it is
        // cleared BEFORE sending, as in the original — otherwise on a daemon
        // refusal the panel would read the playlists of the previous answer.
        if self.stopped_playlists.as_array().is_some_and(|a| !a.is_empty()) {
            self.as_mut().rust_mut().stopped_playlists = Value::Array(Vec::new());
            self.as_mut().stopped_playlists_changed();
        }
        // There is exactly one input — `targets`, as in the original: the v0.3.5
        // `WallpaperApplyRequest` has no other addressing field. An empty list is a
        // legitimate value, not "nothing set": the daemon treats it as "every
        // canvas once plus every standalone display".
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
            // Warning: exactly what Python lost: the name of the renderer that came up.
            // The signal is UNCONDITIONAL, as in the original (wallpaper_query.cpp:539):
            // it means "the daemon answered", not "the value changed".
            let mut rust = self.as_mut().rust_mut();
            rust.stopped_playlists = we_wallpaper::stopped_playlists(&wa);
            rust.renderer_id = wa.renderer_id;
            signals.push(QuerySignal::Property("rendererId"));
            // Also unconditional: the panel reads the list on the status transition,
            // and both properties must be fresh BEFORE it.
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
// WallpaperGetQuery — a fresh view of one wallpaper for the details panel
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperGetQueryRust {
    base: QueryState,
    data: we_wallpaper::WallpaperGetData,
    wallpaper_id: String,
    /// token → which item was asked with it. An answer to a stale request is
    /// dropped — otherwise the panel shows someone else's wallpaper.
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
        // The original's connect_requet_reload: an item change → a deferred reload.
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
        // Warning: protection against a stale answer (`wallpaper_query.cpp:311`):
        // while the answer was on its way, the panel may have switched to another
        // card.
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
// WallpaperRemoveQuery — remove wallpapers from the library
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperRemoveQueryRust {
    base: QueryState,
    wallpaper_id: String,
    /// token → what exactly was asked to be removed: `removedMany` must return
    /// the same set to QML.
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
        // Empty identifiers are filtered out — Python sent them to the daemon as
        // is.
        let ids: Vec<String> = wallpaper_ids
            .iter()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty() {
            return;
        }
        // A single item goes into `wallpaper_id`, not as a list — as in the
        // original.
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

    /// The original's `reload() == remove([wallpaperId])`. Our Python had no
    /// override at all, and the property was dead.
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
                // The daemon refused — there is no removal report at all.
                self.dispatch(signals);
                return;
            }
            Err(_) => {
                let signals = decode_failed(&mut self.as_mut().rust_mut().base);
                self.dispatch(signals);
                return;
            }
        };
        // Warning: the number is taken from the daemon's ANSWER, not from the
        // length of the sent set: Python promised "N removed" however many the
        // daemon actually removed.
        let removed_count = match payload {
            Some(pb::response::Payload::WallpaperRemove(wr)) => wr.removed_count as i32,
            _ => 0,
        };
        // Warning: the report goes out BEFORE switching to Finished: in the
        // original it is emitted inside the parsing lambda, and the status is set
        // after it. While it was the other way round, the QML handler on
        // `finished` fired before the page learned what and how much was removed.
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
// WallpaperScanQuery — rescan the library
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
            // The signal is UNCONDITIONAL (wallpaper_query.cpp:270): a rescan with the
            // same number of wallpapers is still the event "the daemon finished
            // counting".
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
// WallpaperPropertySetQuery — write one wallpaper property
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
        // v0.2.5: an empty `value` is a valid override (text properties); a reset
        // to the value from project.json is ONLY `reset = true`.
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
        // The answer has no payload: success is the absence of an error.
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
// WallpaperLayoutSetQuery — the personal layout of one wallpaper
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
        // waywallen 0.4.4: the original's `m_flip { 1 }` is FLIP_NONE, not "not
        // set": a wallpaper layout always carries an explicit flip.
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

    /// The comparison and the signal are computed by the core — in the original
    /// this is the WW_SET macro.
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
        // `None` means no wallpaper is selected: the original returns BEFORE
        // switching the status, that is, the request does not start at all.
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
// WallpaperApplyViaPortalQuery — hand a wallpaper over through the desktop portal
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
            // The signal is unconditional — wallpaper_query.cpp:574.
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

    /// A frame with a target display must match the one the live 0.3.5 daemon
    /// accepted in the measurement of 18.08 (wallpaper 71 went onto display 2).
    /// The same daemon silently drops a frame with packed `display_ids`
    /// (`0a0237312a0102`) — which is why the targets are built by the v0.3.5
    /// scheme.
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

    /// Display numbers passed through the core's `display_targets` give the
    /// same frame as target maps from QML: the target building engine is one
    /// for the whole tab (rule #12), and there is no second form of addressing.
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
// WallpaperHideQuery — hide wallpapers from the library (waywallen 0.4.4)
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct WallpaperHideQueryRust {
    base: QueryState,
    wallpaper_id: String,
    hidden: bool,
    /// token → (what was asked, in which direction): `updated` must return to
    /// QML exactly what was sent, even if the properties changed since.
    pending: std::collections::BTreeMap<i64, (Vec<String>, bool)>,
}

impl_action_base!(WallpaperHideQuery);

impl cxx_qt::Initialize for qobject::WallpaperHideQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        // The original's `m_hidden { true }`: a bare `reload()` hides.
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
        // The report goes out BEFORE switching to Finished — as in
        // `WallpaperRemoveQuery`: in the original `updated` is emitted inside the
        // parsing lambda.
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
