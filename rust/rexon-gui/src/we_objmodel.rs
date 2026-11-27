// App's live models: renderers, displays, libraries and GPUs.
//
// A port of waywallen's `ui/src/objmodel/{renderer,display,library}.cpp`
// (MIT, (c) hypengw; attribution in NOTICE).
//
// All five classes are in ONE bridge on purpose: cxx emits a shim wrapper
// for every `extern "C++"` declaration, named after the C++ function, and
// the child object factory (`rexon_new_renderer`/`rexon_new_display`)
// declared in two bridges of the crate would give a `duplicate symbol` at
// link time. cxx-qt does not require one `#[qobject]` per file — only the
// `Self` shortcut is lost, which we do not use here anyway.
//
// Who feeds the models: daemon events (`*_snapshot`/`*_changed`/`*_removed`)
// come here as a raw frame through App, plus the `renderer_list` answer
// from `status_queries::RendererListQuery` — as the same raw frame through
// the `renderers_replace_frame` seam.
//
// What came with the v0.3.5 schema and lives here: the renderer state
// (`RendererState` instead of the `status` string), its conditions and tags
// (`runtimeConditions`/`runtimeTags`), and for a display — its canvas
// membership (`canvasId`/`canvasRect`/`canvasOverlapCount`), the settings
// key and its own conditions. The `status` string and `running` are
// assembled by the core (`we_query::renderer_status`) — one engine for
// both arrival paths.
//
// Canvases (`Canvas`) are here too: the daemon sends `canvas_snapshot` both
// on connection and on every change, so there is no separate list request.
// The canvas data (members, extent, layout) come as a snapshot; the links,
// the playlist and the conditions of a canvas are assembled from its
// displays and recomputed after every change of displays or playlists
// (`refresh_canvas_runtime`).
//
// GPUs (`Gpu`/`GpuManager`) are the only model without events: the daemon
// lists the cards once at start-up, and the list comes as the `gpu_list`
// answer, which App requests on every connection (`we_app.rs`).
//
// Current wallpaper displays (`PresentationManager` and its row model
// `NowPlayingModel`) — the "Current wallpapers" card in the tab rail. The
// daemon sends the display snapshot on connection and on every change; a
// disconnection clears the list at once. The wallpaper (which App
// additionally requests with `wallpaper_lookup`) and the target label —
// the names of displays and canvases — are attached to a row.

/// cxx-qt bridge: Renderer, RendererManager, Display, Canvas, DisplayManager,
/// LibraryManager, Gpu, GpuManager, NowPlayingModel, PresentationManager.
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

        include!(<QtCore/QAbstractListModel>);
        /// Base of the card's row model. Without `#[qobject]` — as in the crate's
        /// other bridges with models (`remote_list_model.rs`): otherwise the type
        /// cast shims would be emitted a second time and would not link.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    unsafe extern "C++" {
        include!("rexon_we_objmodel.h");

        /// Create a child `Renderer`/`Display`/`Canvas`/`Gpu` — `new` lives only in
        /// C++.
        ///
        /// # Safety
        /// `parent` is a live QObject manager (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_renderer(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_display(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_canvas(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_gpu(parent: *mut QObject) -> *mut QObject;
        /// Row model of the "Current wallpapers" card — a child of its manager.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_now_playing_model(parent: *mut QObject) -> *mut QObject;

        /// A string of the vendor translation catalogue by its context — the
        /// "Display #…" and "Canvas …" labels for targets that are not in the
        /// lists.
        fn rexon_we_vendor_tr(context: &QString, source: &QString) -> QString;

        /// Destroy a child object deferred (`deleteLater`), as in the original: it
        /// must not be deleted right now — there may be live QML bindings to the
        /// object in the current frame.
        ///
        /// # Safety
        /// `object` is a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_delete_child_later(object: *mut QObject);

        /// A pointer to a QObject as a QVariant value: this is how the vendored QML
        /// gets the `renderers`/`displays`/`gpus` lists.
        ///
        /// # Safety
        /// `object` is a live QObject or nullptr.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_object_to_variant(object: *mut QObject) -> QVariant;
        // (the declaration lives in rexon_we_objmodel.h — next to the factory)
    }

    // ── Renderer ──────────────────────────────────────────────────────────
    extern "RustQt" {
        /// One renderer as a QObject (a mirror of `proto::RendererInstance`).
        /// Created and destroyed only together with the manager — an instance cannot
        /// be assembled from QML, as in the original (`QML_UNCREATABLE`).
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(QString, id, READ = id, CONSTANT)]
        #[qproperty(u32, fps, READ = fps, NOTIFY = fps_changed)]
        #[qproperty(QString, status, READ = status, NOTIFY = status_changed)]
        /// A live process: playing, paused or muted. A port of the vendor's
        /// `Renderer::running` — `StatusPage.qml:347` colours the icon with it, and
        /// `:331` hides held-back processes.
        #[qproperty(bool, running, READ = running, NOTIFY = status_changed)]
        /// The process is held back after stopping (`Renderer::keep`).
        #[qproperty(bool, keep, READ = keep, NOTIFY = status_changed)]
        /// Reason of the last exit (`Renderer::lastExitReason`).
        #[qproperty(QString, last_exit_reason, cxx_name = "lastExitReason",
                    READ = last_exit_reason, NOTIFY = status_changed)]
        /// Process generation (`Renderer::processGeneration`).
        /// Warning: `f64`, not `u64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`). The field inside stays `u64`.
        #[qproperty(f64, process_generation, cxx_name = "processGeneration",
                    READ = process_generation, NOTIFY = status_changed)]
        /// What is going on with the renderer right now: loading, waiting, hanging
        /// (`StatusPage.qml:281`). A port of `Renderer::runtimeConditions`.
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = runtime_conditions, NOTIFY = runtime_conditions_changed)]
        /// Key-value pairs from the renderer (`StatusPage.qml`, badges).
        #[qproperty(QList_QVariant, runtime_tags, cxx_name = "runtimeTags",
                    READ = runtime_tags, NOTIFY = runtime_tags_changed)]
        #[qproperty(QString, name, READ = name, NOTIFY = name_changed)]
        #[qproperty(u32, pid, READ = pid, NOTIFY = pid_changed)]
        #[qproperty(u32, texture_width, cxx_name = "textureWidth",
                    READ = texture_width, NOTIFY = texture_size_changed)]
        #[qproperty(u32, texture_height, cxx_name = "textureHeight",
                    READ = texture_height, NOTIFY = texture_size_changed)]
        /// The renderer's card (DRM node number). Warning: NOT a constant property:
        /// the renderer is born with zeros and names its card later, with an
        /// update — the icon on an open "Status" page waits for exactly this signal
        /// (the original's `drmRenderChanged`).
        #[qproperty(u32, drm_render_major, cxx_name = "drmRenderMajor",
                    READ = drm_render_major, NOTIFY = drm_render_changed)]
        #[qproperty(u32, drm_render_minor, cxx_name = "drmRenderMinor",
                    READ = drm_render_minor, NOTIFY = drm_render_changed)]
        type Renderer = super::RendererRust;

        fn id(self: &Renderer) -> QString;
        fn fps(self: &Renderer) -> u32;
        fn status(self: &Renderer) -> QString;
        fn running(self: &Renderer) -> bool;
        fn keep(self: &Renderer) -> bool;
        #[cxx_name = "lastExitReason"]
        fn last_exit_reason(self: &Renderer) -> QString;
        #[cxx_name = "processGeneration"]
        fn process_generation(self: &Renderer) -> f64;
        #[cxx_name = "runtimeConditions"]
        fn runtime_conditions(self: &Renderer) -> QList_QVariant;
        #[cxx_name = "runtimeTags"]
        fn runtime_tags(self: &Renderer) -> QList_QVariant;
        fn name(self: &Renderer) -> QString;
        fn pid(self: &Renderer) -> u32;
        #[cxx_name = "textureWidth"]
        fn texture_width(self: &Renderer) -> u32;
        #[cxx_name = "textureHeight"]
        fn texture_height(self: &Renderer) -> u32;
        #[cxx_name = "drmRenderMajor"]
        fn drm_render_major(self: &Renderer) -> u32;
        #[cxx_name = "drmRenderMinor"]
        fn drm_render_minor(self: &Renderer) -> u32;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "fpsChanged"]
        fn fps_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "nameChanged"]
        fn name_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "pidChanged"]
        fn pid_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "textureSizeChanged"]
        fn texture_size_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "drmRenderChanged"]
        fn drm_render_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "runtimeConditionsChanged"]
        fn runtime_conditions_changed(self: Pin<&mut Renderer>);
        #[qsignal]
        #[cxx_name = "runtimeTagsChanged"]
        fn runtime_tags_changed(self: Pin<&mut Renderer>);
    }

    // ── RendererManager ───────────────────────────────────────────────────
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QList_QVariant, renderers, READ = renderers,
                    NOTIFY = renderers_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = renderers_changed)]
        type RendererManager = super::RendererManagerRust;

        fn renderers(self: &RendererManager) -> QList_QVariant;
        fn count(self: &RendererManager) -> i32;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "renderersChanged"]
        fn renderers_changed(self: Pin<&mut RendererManager>);

        /// Renderer by identifier or nullptr.
        #[qinvokable]
        fn get(self: &RendererManager, id: &QString) -> *mut QObject;
    }

    // ── Display ───────────────────────────────────────────────────────────
    extern "RustQt" {
        /// One display as a QObject (a mirror of `proto::DisplayInfo`).
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        /// Warning: `f64`, not `u64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`). The daemon's display numbers are small,
        /// there is no loss of precision.
        #[qproperty(f64, id, cxx_name = "id", READ = display_id, CONSTANT)]
        #[qproperty(QString, name, READ = display_name, NOTIFY = name_changed)]
        #[qproperty(QString, alias, READ = alias, NOTIFY = alias_changed)]
        #[qproperty(QString, display_label, cxx_name = "displayLabel",
                    READ = display_label, NOTIFY = display_label_changed)]
        #[qproperty(u32, width, READ = width, NOTIFY = size_changed)]
        #[qproperty(u32, height, READ = height, NOTIFY = size_changed)]
        #[qproperty(u32, refresh_mhz, cxx_name = "refreshMhz",
                    READ = refresh_mhz, NOTIFY = refresh_mhz_changed)]
        #[qproperty(QList_QVariant, links, READ = links, NOTIFY = links_changed)]
        /// waywallen 0.4.2: the display was paused by hand (with the button of the
        /// "Displays" page, `DisplayPauseSetQuery`).
        #[qproperty(bool, manual_paused, cxx_name = "manualPaused",
                    READ = manual_paused, NOTIFY = pause_changed)]
        /// waywallen 0.4.2: the display is paused for any reason — by hand or by the
        /// auto-pause rules.
        #[qproperty(bool, effective_paused, cxx_name = "effectivePaused",
                    READ = effective_paused, NOTIFY = pause_changed)]
        #[qproperty(QMap_QString_QVariant, effective_layout, cxx_name = "effectiveLayout",
                    READ = effective_layout, NOTIFY = layout_changed)]
        #[qproperty(QMap_QString_QVariant, display_layout, cxx_name = "displayLayout",
                    READ = display_layout, NOTIFY = layout_changed)]
        #[qproperty(QMap_QString_QVariant, layout_override, cxx_name = "layoutOverride",
                    READ = layout_override, NOTIFY = layout_changed)]
        #[qproperty(bool, layout_overridden_by_wallpaper,
                    cxx_name = "layoutOverriddenByWallpaper",
                    READ = layout_overridden_by_wallpaper, NOTIFY = layout_changed)]
        #[qproperty(u32, drm_render_major, cxx_name = "drmRenderMajor",
                    READ = drm_render_major, CONSTANT)]
        #[qproperty(u32, drm_render_minor, cxx_name = "drmRenderMinor",
                    READ = drm_render_minor, CONSTANT)]
        /// Warning: `f64`, not `i64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`).
        #[qproperty(f64, active_playlist_id, cxx_name = "activePlaylistId",
                    READ = active_playlist_id, NOTIFY = playlist_status_changed)]
        #[qproperty(QMap_QString_QVariant, playlist_status, cxx_name = "playlistStatus",
                    READ = playlist_status, NOTIFY = playlist_status_changed)]
        /// Whether the display is fit to be a separate apply target. In the daemon
        /// this is `selectable_target = canvas_id.is_none()` (router/mod.rs:2666):
        /// a display given to a canvas is not offered as a target — the target is
        /// then the canvas. We repeat the formula by reading `canvas_id` (see
        /// `DisplayInfo`).
        #[qproperty(bool, selectable_target, cxx_name = "selectableTarget",
                    READ = selectable_target, NOTIFY = canvas_changed)]
        /// The canvas the display is given to ("" — the display is on its own).
        #[qproperty(QString, canvas_id, cxx_name = "canvasId",
                    READ = canvas_id, NOTIFY = canvas_changed)]
        /// The display's place inside the canvas (`x`/`y`/`width`/`height`).
        #[qproperty(QMap_QString_QVariant, canvas_rect, cxx_name = "canvasRect",
                    READ = canvas_rect, NOTIFY = canvas_changed)]
        /// How many canvas neighbours overlap this display.
        #[qproperty(u32, canvas_overlap_count, cxx_name = "canvasOverlapCount",
                    READ = canvas_overlap_count, NOTIFY = canvas_changed)]
        /// The display's key in the daemon settings — the `config.toml` section is
        /// labelled with it, and the canvas lists its members by it too.
        #[qproperty(QString, settings_key, cxx_name = "settingsKey",
                    READ = settings_key, NOTIFY = settings_key_changed)]
        /// What is going on with the display right now (`DisplaysPage.qml:302`).
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = runtime_conditions, NOTIFY = runtime_conditions_changed)]
        /// waywallen 0.4.4: whether the display client supports window exclusions
        /// (`WindowExclusionSupport`).
        #[qproperty(i32, window_exclusion_support, cxx_name = "windowExclusionSupport",
                    READ = window_exclusion_support,
                    NOTIFY = window_exclusion_support_changed)]
        /// waywallen 0.4.4: how many exclusion rules the display will not apply —
        /// by it the "Excluded windows" page warns the user.
        #[qproperty(u32, unsupported_window_exclusions,
                    cxx_name = "unsupportedWindowExclusions",
                    READ = unsupported_window_exclusions,
                    NOTIFY = window_exclusion_support_changed)]
        type Display = super::DisplayRust;

        #[cxx_name = "id"]
        fn display_id(self: &Display) -> f64;
        #[cxx_name = "name"]
        fn display_name(self: &Display) -> QString;
        fn alias(self: &Display) -> QString;
        #[cxx_name = "displayLabel"]
        fn display_label(self: &Display) -> QString;
        fn width(self: &Display) -> u32;
        fn height(self: &Display) -> u32;
        #[cxx_name = "refreshMhz"]
        fn refresh_mhz(self: &Display) -> u32;
        fn links(self: &Display) -> QList_QVariant;
        #[cxx_name = "manualPaused"]
        fn manual_paused(self: &Display) -> bool;
        #[cxx_name = "effectivePaused"]
        fn effective_paused(self: &Display) -> bool;
        #[cxx_name = "effectiveLayout"]
        fn effective_layout(self: &Display) -> QMap_QString_QVariant;
        #[cxx_name = "displayLayout"]
        fn display_layout(self: &Display) -> QMap_QString_QVariant;
        #[cxx_name = "layoutOverride"]
        fn layout_override(self: &Display) -> QMap_QString_QVariant;
        #[cxx_name = "layoutOverriddenByWallpaper"]
        fn layout_overridden_by_wallpaper(self: &Display) -> bool;
        #[cxx_name = "drmRenderMajor"]
        fn drm_render_major(self: &Display) -> u32;
        #[cxx_name = "drmRenderMinor"]
        fn drm_render_minor(self: &Display) -> u32;
        #[cxx_name = "activePlaylistId"]
        fn active_playlist_id(self: &Display) -> f64;
        #[cxx_name = "playlistStatus"]
        fn playlist_status(self: &Display) -> QMap_QString_QVariant;
        #[cxx_name = "selectableTarget"]
        fn selectable_target(self: &Display) -> bool;
        #[cxx_name = "canvasId"]
        fn canvas_id(self: &Display) -> QString;
        #[cxx_name = "canvasRect"]
        fn canvas_rect(self: &Display) -> QMap_QString_QVariant;
        #[cxx_name = "canvasOverlapCount"]
        fn canvas_overlap_count(self: &Display) -> u32;
        #[cxx_name = "settingsKey"]
        fn settings_key(self: &Display) -> QString;
        #[cxx_name = "runtimeConditions"]
        fn runtime_conditions(self: &Display) -> QList_QVariant;
        #[cxx_name = "windowExclusionSupport"]
        fn window_exclusion_support(self: &Display) -> i32;
        #[cxx_name = "unsupportedWindowExclusions"]
        fn unsupported_window_exclusions(self: &Display) -> u32;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "nameChanged"]
        fn name_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "aliasChanged"]
        fn alias_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "displayLabelChanged"]
        fn display_label_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "sizeChanged"]
        fn size_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "refreshMhzChanged"]
        fn refresh_mhz_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "linksChanged"]
        fn links_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "pauseChanged"]
        fn pause_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "layoutChanged"]
        fn layout_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "playlistStatusChanged"]
        fn playlist_status_changed(self: Pin<&mut Display>);
        /// A change of canvas membership: in the original
        /// `canvasId`/`canvasRect`/`canvasOverlapCount`/`selectableTarget` hang on
        /// this signal.
        #[qsignal]
        #[cxx_name = "canvasChanged"]
        fn canvas_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "settingsKeyChanged"]
        fn settings_key_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "runtimeConditionsChanged"]
        fn runtime_conditions_changed(self: Pin<&mut Display>);
        #[qsignal]
        #[cxx_name = "windowExclusionSupportChanged"]
        fn window_exclusion_support_changed(self: Pin<&mut Display>);
    }

    // ── Canvas ────────────────────────────────────────────────────────────
    extern "RustQt" {
        /// One canvas as a QObject (a mirror of `proto::CanvasInfo`): several
        /// displays onto which a wallpaper lies as one sheet. Not created from QML —
        /// the canvases are owned by `DisplayManager`.
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(QString, id, cxx_name = "id", READ = canvas_object_id, CONSTANT)]
        #[qproperty(QString, name, READ = canvas_name, NOTIFY = changed)]
        /// The same name: a canvas, unlike a display, has no alias.
        #[qproperty(QString, display_label, cxx_name = "displayLabel",
                    READ = canvas_name, NOTIFY = changed)]
        /// Canvas members: `settingsKey`, `rect`, `minimumScaleTo`, `aspectLocked`,
        /// `displayIds`, `onlineCount`, `overlap`.
        #[qproperty(QList_QVariant, members, READ = members, NOTIFY = changed)]
        /// Canvas extent (`x`/`y`/`width`/`height`); if the daemon did not send it,
        /// the map is empty.
        #[qproperty(QMap_QString_QVariant, extent, READ = extent, NOTIFY = changed)]
        #[qproperty(u32, width, READ = extent_width, NOTIFY = changed)]
        #[qproperty(u32, height, READ = extent_height, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, layout_override, cxx_name = "layoutOverride",
                    READ = canvas_layout_override, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, effective_layout, cxx_name = "effectiveLayout",
                    READ = canvas_effective_layout, NOTIFY = changed)]
        /// Links to renderers, merged over all displays of the canvas.
        #[qproperty(QList_QVariant, links, READ = canvas_links, NOTIFY = runtime_changed)]
        /// Warning: `f64`, not `i64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`).
        #[qproperty(f64, active_playlist_id, cxx_name = "activePlaylistId",
                    READ = canvas_active_playlist_id, NOTIFY = runtime_changed)]
        #[qproperty(QMap_QString_QVariant, playlist_status, cxx_name = "playlistStatus",
                    READ = canvas_playlist_status, NOTIFY = runtime_changed)]
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = canvas_runtime_conditions, NOTIFY = runtime_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId",
                    READ = wallpaper_id, NOTIFY = changed)]
        /// Canvas revision number: a change and a removal go out with it, and the
        /// daemon refuses if the canvas has been changed meanwhile. Warning: `f64`,
        /// not `u64` — for the same reason as above.
        #[qproperty(f64, revision, READ = revision, NOTIFY = changed)]
        #[qproperty(i32, member_count, cxx_name = "memberCount",
                    READ = member_count, NOTIFY = changed)]
        #[qproperty(i32, online_count, cxx_name = "onlineCount",
                    READ = online_count, NOTIFY = changed)]
        #[qproperty(bool, has_live_displays, cxx_name = "hasLiveDisplays",
                    READ = has_live_displays, NOTIFY = changed)]
        #[qproperty(bool, empty, READ = empty, NOTIFY = changed)]
        type Canvas = super::CanvasRust;

        #[cxx_name = "id"]
        fn canvas_object_id(self: &Canvas) -> QString;
        #[cxx_name = "name"]
        fn canvas_name(self: &Canvas) -> QString;
        fn members(self: &Canvas) -> QList_QVariant;
        fn extent(self: &Canvas) -> QMap_QString_QVariant;
        #[cxx_name = "width"]
        fn extent_width(self: &Canvas) -> u32;
        #[cxx_name = "height"]
        fn extent_height(self: &Canvas) -> u32;
        #[cxx_name = "layoutOverride"]
        fn canvas_layout_override(self: &Canvas) -> QMap_QString_QVariant;
        #[cxx_name = "effectiveLayout"]
        fn canvas_effective_layout(self: &Canvas) -> QMap_QString_QVariant;
        #[cxx_name = "links"]
        fn canvas_links(self: &Canvas) -> QList_QVariant;
        #[cxx_name = "activePlaylistId"]
        fn canvas_active_playlist_id(self: &Canvas) -> f64;
        #[cxx_name = "playlistStatus"]
        fn canvas_playlist_status(self: &Canvas) -> QMap_QString_QVariant;
        #[cxx_name = "runtimeConditions"]
        fn canvas_runtime_conditions(self: &Canvas) -> QList_QVariant;
        #[cxx_name = "wallpaperId"]
        fn wallpaper_id(self: &Canvas) -> QString;
        fn revision(self: &Canvas) -> f64;
        #[cxx_name = "memberCount"]
        fn member_count(self: &Canvas) -> i32;
        #[cxx_name = "onlineCount"]
        fn online_count(self: &Canvas) -> i32;
        #[cxx_name = "hasLiveDisplays"]
        fn has_live_displays(self: &Canvas) -> bool;
        fn empty(self: &Canvas) -> bool;
    }

    unsafe extern "RustQt" {
        /// What the daemon sent about the canvas itself changed.
        #[qsignal]
        fn changed(self: Pin<&mut Canvas>);
        /// What is assembled from the canvas displays changed: links, playlist,
        /// conditions.
        #[qsignal]
        #[cxx_name = "runtimeChanged"]
        fn runtime_changed(self: Pin<&mut Canvas>);
    }

    // ── DisplayManager ────────────────────────────────────────────────────
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QList_QVariant, displays, READ = displays,
                    NOTIFY = displays_changed)]
        /// Canvases by name, then by identifier (`Canvas` objects).
        #[qproperty(QList_QVariant, canvases, READ = canvases,
                    NOTIFY = canvases_changed)]
        /// Revision number of the whole set of canvases from the daemon's last
        /// snapshot. Warning: `f64`, not `u64` (the analysis is in `src/qvalue.rs`).
        #[qproperty(f64, canvas_revision, cxx_name = "canvasRevision",
                    READ = canvas_revision, NOTIFY = canvases_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = displays_changed)]
        #[qproperty(bool, has_active_playlist_displays,
                    cxx_name = "hasActivePlaylistDisplays",
                    READ = has_active_playlist_displays,
                    NOTIFY = playlist_status_changed)]
        type DisplayManager = super::DisplayManagerRust;

        fn displays(self: &DisplayManager) -> QList_QVariant;
        fn canvases(self: &DisplayManager) -> QList_QVariant;
        #[cxx_name = "canvasRevision"]
        fn canvas_revision(self: &DisplayManager) -> f64;
        fn count(self: &DisplayManager) -> i32;
        #[cxx_name = "hasActivePlaylistDisplays"]
        fn has_active_playlist_displays(self: &DisplayManager) -> bool;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "displaysChanged"]
        fn displays_changed(self: Pin<&mut DisplayManager>);
        #[qsignal]
        #[cxx_name = "canvasesChanged"]
        fn canvases_changed(self: Pin<&mut DisplayManager>);
        #[qsignal]
        #[cxx_name = "playlistStatusChanged"]
        fn playlist_status_changed(self: Pin<&mut DisplayManager>);

        /// Display by identifier or nullptr.
        ///
        /// Warning: we take `id` as `f64`: we do not expose a 64-bit integer in a
        /// signature visible to QML (the analysis is in `src/qvalue.rs`). The
        /// vendored QML calls `W.App.displayManager.get(displayId)` with an ordinary
        /// JS number.
        #[qinvokable]
        fn get(self: &DisplayManager, id: f64) -> *mut QObject;

        /// Canvas by identifier, or nullptr for an unknown id.
        #[qinvokable]
        #[cxx_name = "getCanvas"]
        fn get_canvas(self: &DisplayManager, id: &QString) -> *mut QObject;
    }

    // ── Gpu ───────────────────────────────────────────────────────────────
    extern "RustQt" {
        /// One GPU (a mirror of `proto::GpuInfo`). The daemon lists the cards once
        /// at start-up and reports nothing more about them, so all properties are
        /// constant — as in the original.
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(QString, render_node, cxx_name = "renderNode",
                    READ = render_node, CONSTANT)]
        #[qproperty(QString, primary_node, cxx_name = "primaryNode",
                    READ = primary_node, CONSTANT)]
        #[qproperty(u32, render_major, cxx_name = "renderMajor",
                    READ = render_major, CONSTANT)]
        #[qproperty(u32, render_minor, cxx_name = "renderMinor",
                    READ = render_minor, CONSTANT)]
        #[qproperty(u32, primary_major, cxx_name = "primaryMajor",
                    READ = primary_major, CONSTANT)]
        #[qproperty(u32, primary_minor, cxx_name = "primaryMinor",
                    READ = primary_minor, CONSTANT)]
        #[qproperty(QString, pci_bdf, cxx_name = "pciBdf", READ = pci_bdf, CONSTANT)]
        #[qproperty(u32, vendor_id, cxx_name = "vendorId", READ = vendor_id, CONSTANT)]
        #[qproperty(u32, device_id, cxx_name = "deviceId", READ = device_id, CONSTANT)]
        #[qproperty(QString, driver, READ = driver, CONSTANT)]
        #[qproperty(QString, name, READ = gpu_name, CONSTANT)]
        #[qproperty(QString, description, READ = description, CONSTANT)]
        type Gpu = super::GpuRust;

        #[cxx_name = "renderNode"]
        fn render_node(self: &Gpu) -> QString;
        #[cxx_name = "primaryNode"]
        fn primary_node(self: &Gpu) -> QString;
        #[cxx_name = "renderMajor"]
        fn render_major(self: &Gpu) -> u32;
        #[cxx_name = "renderMinor"]
        fn render_minor(self: &Gpu) -> u32;
        #[cxx_name = "primaryMajor"]
        fn primary_major(self: &Gpu) -> u32;
        #[cxx_name = "primaryMinor"]
        fn primary_minor(self: &Gpu) -> u32;
        #[cxx_name = "pciBdf"]
        fn pci_bdf(self: &Gpu) -> QString;
        #[cxx_name = "vendorId"]
        fn vendor_id(self: &Gpu) -> u32;
        #[cxx_name = "deviceId"]
        fn device_id(self: &Gpu) -> u32;
        fn driver(self: &Gpu) -> QString;
        #[cxx_name = "name"]
        fn gpu_name(self: &Gpu) -> QString;
        fn description(self: &Gpu) -> QString;
    }

    // ── GpuManager ────────────────────────────────────────────────────────
    extern "RustQt" {
        /// The machine's GPUs in the daemon's order (`Gpu` objects): the card icon
        /// at the renderer and at the display, choosing a card in the settings.
        #[qobject]
        #[qml_element]
        #[qproperty(QList_QVariant, gpus, READ = gpus, NOTIFY = gpus_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = gpus_changed)]
        type GpuManager = super::GpuManagerRust;

        fn gpus(self: &GpuManager) -> QList_QVariant;
        fn count(self: &GpuManager) -> i32;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "gpusChanged"]
        fn gpus_changed(self: Pin<&mut GpuManager>);

        /// Card by DRM node number (the render node or the primary one) or nullptr:
        /// the node is unknown (`major == 0`) or there is no such card.
        #[qinvokable]
        fn find(self: &GpuManager, major: u32, minor: u32) -> *mut QObject;
    }

    // ── NowPlayingModel ───────────────────────────────────────────────────
    extern "RustQt" {
        /// Rows of the "Current wallpapers" card: the `wallpaper`, `wallpaperId`,
        /// `targetSummary`, `state` roles — read by
        /// `component/SidebarNowPlaying.qml`. Lives only at its manager.
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type NowPlayingModel = super::NowPlayingModelRust;

        fn count(self: &NowPlayingModel) -> i32;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &NowPlayingModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &NowPlayingModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &NowPlayingModel, index: &QModelIndex, role: i32) -> QVariant;

        /// The whole row — this is how the card learns which wallpaper to open.
        #[qinvokable]
        fn item(self: &NowPlayingModel, row: i32) -> QMap_QString_QVariant;

        /// # Safety
        /// Requires a paired `end_reset_model`.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut NowPlayingModel>);
        /// # Safety
        /// Paired with `begin_reset_model`.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut NowPlayingModel>);
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut NowPlayingModel>);

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut NowPlayingModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        #[inherit]
        fn index(
            self: &NowPlayingModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── PresentationManager ───────────────────────────────────────────────
    extern "RustQt" {
        /// Current wallpaper displays: `count` and `model` — exactly what the rail
        /// reads (`W.App.presentationManager`).
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        #[qproperty(*mut QObject, model, READ = model, CONSTANT)]
        type PresentationManager = super::PresentationManagerRust;

        fn count(self: &PresentationManager) -> i32;
    }

    unsafe extern "RustQt" {
        fn model(self: &PresentationManager) -> *mut QObject;

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut PresentationManager>);
    }

    // ── LibraryManager ────────────────────────────────────────────────────
    extern "RustQt" {
        /// List of wallpaper libraries (SourceManagePage).
        ///
        /// Warning: in the original a list row is a `Library` QObject with four
        /// properties; here it stays a map. There is no observable difference: the
        /// vendored QML reads only `id`/`name`/`path`/`pluginName` of a row with a
        /// dot, and a map answers that exactly the same way. Creating a QObject per
        /// row without a single signal would be an extra lifecycle node for the same
        /// result. A name change (`LibraryChanged`) still arrives as
        /// `librariesChanged` — the list is rebuilt entirely.
        ///
        /// `initialized` (waywallen 0.5.0) — whether the first full list has
        /// already arrived: before it "no libraries" means "we do not know yet", and
        /// SourceManagePage does not show the empty state.
        #[qobject]
        #[qml_element]
        #[qproperty(QList_QVariant, libraries, READ = libraries,
                    NOTIFY = libraries_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = libraries_changed)]
        #[qproperty(bool, initialized, READ = initialized, NOTIFY = libraries_changed)]
        type LibraryManager = super::LibraryManagerRust;

        fn libraries(self: &LibraryManager) -> QList_QVariant;
        fn count(self: &LibraryManager) -> i32;
        fn initialized(self: &LibraryManager) -> bool;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "librariesChanged"]
        fn libraries_changed(self: Pin<&mut LibraryManager>);
        /// A separate signal of our bridge, kept for the bindings that listened to
        /// exactly it.
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut LibraryManager>);
    }
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use std::collections::BTreeMap;

use prost::Message;
use rexon_core::proto::control as pb;
use rexon_core::we_query as core_weq;

// ============================================================================
// Renderer
// ============================================================================

/// `RendererInstance` fields that arrive by two paths: a daemon event
/// (protobuf) and the `renderer_list` answer (a map from the Python query).
/// One normalizer for both — rule #12.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererInfo {
    pub id: String,
    pub fps: u32,
    /// The status string. Since v0.3.5 the daemon does not send it — it is
    /// assembled from the `RendererState` tree by the core's shared engine
    /// (`we_query::renderer_status`), one for all renderer arrival paths.
    pub status: String,
    pub running: bool,
    pub keep: bool,
    pub last_exit_reason: String,
    pub process_generation: u64,
    pub conditions: Vec<core_weq::RuntimeCondition>,
    pub runtime_tags: Vec<core_weq::RuntimeTag>,
    pub name: String,
    pub pid: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub drm_render_major: u32,
    pub drm_render_minor: u32,
}

impl RendererInfo {
    fn from_pb(info: &pb::RendererInstance) -> Self {
        let status = core_weq::renderer_status(info);
        Self {
            id: info.renderer_id.clone(),
            fps: info.fps,
            status: status.to_string(),
            running: core_weq::renderer_running(status),
            keep: core_weq::renderer_keep(info),
            last_exit_reason: core_weq::renderer_last_exit_reason(info),
            process_generation: core_weq::renderer_process_generation(info),
            conditions: core_weq::runtime_conditions(&info.conditions),
            runtime_tags: core_weq::runtime_tags(&info.runtime_tags),
            name: info.name.clone(),
            pid: info.pid,
            texture_width: info.texture_width,
            texture_height: info.texture_height,
            drm_render_major: info.drm_render_major,
            drm_render_minor: info.drm_render_minor,
        }
    }
}

/// `RuntimeCondition` → a map for QML (the vendor's keys).
fn conditions_list(list: &[core_weq::RuntimeCondition]) -> qobject::QList_QVariant {
    let mut out = QList::<QVariant>::default();
    for c in list {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        map.insert(QString::from("kind"), QVariant::from(&QString::from(&c.kind)));
        map.insert(
            QString::from("origin"),
            QVariant::from(&QString::from(&c.origin)),
        );
        map.insert(
            QString::from("reason"),
            QVariant::from(&QString::from(&c.reason)),
        );
        map.insert(
            QString::from("relatedRendererId"),
            QVariant::from(&QString::from(&c.related_renderer_id)),
        );
        map.insert(
            QString::from("relatedDisplayId"),
            QVariant::from(&c.related_display_id),
        );
        out.append(QVariant::from(&map));
    }
    out
}

/// `RendererRuntimeTag` → a map for QML.
fn tags_list(list: &[core_weq::RuntimeTag]) -> qobject::QList_QVariant {
    let mut out = QList::<QVariant>::default();
    for t in list {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        map.insert(QString::from("key"), QVariant::from(&QString::from(&t.key)));
        map.insert(
            QString::from("value"),
            QVariant::from(&QString::from(&t.value)),
        );
        out.append(QVariant::from(&map));
    }
    out
}

#[derive(Default)]
pub struct RendererRust {
    info: RendererInfo,
}

impl qobject::Renderer {
    fn id(&self) -> QString {
        QString::from(&self.rust().info.id)
    }
    fn fps(&self) -> u32 {
        self.rust().info.fps
    }
    fn status(&self) -> QString {
        QString::from(&self.rust().info.status)
    }
    fn running(&self) -> bool {
        self.rust().info.running
    }
    fn keep(&self) -> bool {
        self.rust().info.keep
    }
    fn last_exit_reason(&self) -> QString {
        QString::from(&self.rust().info.last_exit_reason)
    }
    fn process_generation(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.rust().info.process_generation)
    }
    fn runtime_conditions(&self) -> qobject::QList_QVariant {
        conditions_list(&self.rust().info.conditions)
    }
    fn runtime_tags(&self) -> qobject::QList_QVariant {
        tags_list(&self.rust().info.runtime_tags)
    }
    fn name(&self) -> QString {
        QString::from(&self.rust().info.name)
    }
    fn pid(&self) -> u32 {
        self.rust().info.pid
    }
    fn texture_width(&self) -> u32 {
        self.rust().info.texture_width
    }
    fn texture_height(&self) -> u32 {
        self.rust().info.texture_height
    }
    fn drm_render_major(&self) -> u32 {
        self.rust().info.drm_render_major
    }
    fn drm_render_minor(&self) -> u32 {
        self.rust().info.drm_render_minor
    }

    /// Initial filling: we send no signals, the object has not been handed to
    /// anyone yet.
    fn install(mut self: Pin<&mut Self>, info: RendererInfo) {
        self.as_mut().rust_mut().info = info;
    }

    /// A port of `Renderer::updateFrom`: a signal is sent ONLY for the property
    /// that changed, not as a general "something changed".
    fn update_from(mut self: Pin<&mut Self>, info: RendererInfo) {
        let old = self.rust().info.clone();
        self.as_mut().rust_mut().info = info;
        let new = self.rust().info.clone();
        if old.fps != new.fps {
            self.as_mut().fps_changed();
        }
        // One signal for the whole state — like the vendor's `stateChanged`: the
        // `status` string may not change (two "stopped" in a row), while the
        // process hold-back or the exit reason may.
        if old.status != new.status
            || old.running != new.running
            || old.keep != new.keep
            || old.last_exit_reason != new.last_exit_reason
            || old.process_generation != new.process_generation
        {
            self.as_mut().status_changed();
        }
        if old.conditions != new.conditions {
            self.as_mut().runtime_conditions_changed();
        }
        if old.runtime_tags != new.runtime_tags {
            self.as_mut().runtime_tags_changed();
        }
        if old.name != new.name {
            self.as_mut().name_changed();
        }
        if old.pid != new.pid {
            self.as_mut().pid_changed();
        }
        if old.texture_width != new.texture_width || old.texture_height != new.texture_height {
            self.as_mut().texture_size_changed();
        }
        if old.drm_render_major != new.drm_render_major
            || old.drm_render_minor != new.drm_render_minor
        {
            self.as_mut().drm_render_changed();
        }
    }
}

// ============================================================================
// RendererManager
// ============================================================================

/// The pointers are raw: Qt owns the objects (they are children of the
/// manager), and the order and lookup are kept here — like the original's
/// `m_ordered`/`m_by_id`.
#[derive(Default)]
pub struct RendererManagerRust {
    by_id: BTreeMap<String, *mut qobject::QObject>,
}

/// # Safety
/// `ptr` is a live `Renderer` created by this bridge's factory.
unsafe fn renderer_pin<'a>(ptr: *mut qobject::QObject) -> Pin<&'a mut qobject::Renderer> {
    unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Renderer>()) }
}

impl qobject::RendererManager {
    fn renderers(&self) -> qobject::QList_QVariant {
        // The BTreeMap is already ordered by identifier — the very `std::sort` by
        // `id` that the original does.
        let mut out = QList::<QVariant>::default();
        for ptr in self.rust().by_id.values() {
            out.append(unsafe { qobject::rexon_object_to_variant(*ptr) });
        }
        out
    }

    fn count(&self) -> i32 {
        self.rust().by_id.len() as i32
    }

    fn get(&self, id: &QString) -> *mut qobject::QObject {
        self.rust()
            .by_id
            .get(&id.to_string())
            .copied()
            .unwrap_or(std::ptr::null_mut())
    }

    /// A port of `RendererManager::replaceAll`: what is not in the snapshot is
    /// removed, the rest is updated, exactly one `renderersChanged` per batch.
    pub(crate) fn replace_all(mut self: Pin<&mut Self>, infos: Vec<RendererInfo>) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::RendererManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::RendererManager).cast()
        };
        let fresh: BTreeMap<String, RendererInfo> =
            infos.into_iter().map(|i| (i.id.clone(), i)).collect();

        let stale: Vec<String> = self
            .rust()
            .by_id
            .keys()
            .filter(|id| !fresh.contains_key(*id))
            .cloned()
            .collect();
        let mut changed = !stale.is_empty();
        for id in stale {
            if let Some(ptr) = self.as_mut().rust_mut().by_id.remove(&id) {
                unsafe { qobject::rexon_delete_child_later(ptr) };
            }
        }

        for (id, info) in fresh {
            let existing = self.rust().by_id.get(&id).copied();
            match existing {
                Some(ptr) => unsafe { renderer_pin(ptr) }.update_from(info),
                None => {
                    let ptr = unsafe { qobject::rexon_new_renderer(parent) };
                    unsafe { renderer_pin(ptr) }.install(info);
                    self.as_mut().rust_mut().by_id.insert(id, ptr);
                    changed = true;
                }
            }
        }

        if changed {
            self.as_mut().renderers_changed();
        }
    }

    /// A port of `RendererManager::upsert`.
    fn upsert(mut self: Pin<&mut Self>, info: RendererInfo) {
        if let Some(ptr) = self.rust().by_id.get(&info.id).copied() {
            unsafe { renderer_pin(ptr) }.update_from(info);
            return;
        }
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::RendererManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::RendererManager).cast()
        };
        let id = info.id.clone();
        let ptr = unsafe { qobject::rexon_new_renderer(parent) };
        unsafe { renderer_pin(ptr) }.install(info);
        self.as_mut().rust_mut().by_id.insert(id, ptr);
        self.as_mut().renderers_changed();
    }

    /// A port of `RendererManager::remove`.
    fn remove(mut self: Pin<&mut Self>, id: &str) {
        let Some(ptr) = self.as_mut().rust_mut().by_id.remove(id) else {
            return;
        };
        unsafe { qobject::rexon_delete_child_later(ptr) };
        self.as_mut().renderers_changed();
    }

    /// Parsing a daemon event (the original's `handleEvent`).
    pub(crate) fn handle_event(self: Pin<&mut Self>, event: &pb::Event) {
        use pb::event::Payload;
        match event.payload.as_ref() {
            Some(Payload::RendererSnapshot(snap)) => {
                self.replace_all(snap.renderers.iter().map(RendererInfo::from_pb).collect());
            }
            Some(Payload::RendererChanged(changed)) => {
                if let Some(info) = changed.renderer.as_ref() {
                    self.upsert(RendererInfo::from_pb(info));
                }
            }
            Some(Payload::RendererRemoved(removed)) => {
                let id = removed.renderer_id.clone();
                self.remove(&id);
            }
            _ => {}
        }
    }

    /// The `renderer_list` answer — a full replacement of the list.
    ///
    /// The input is the same RAW daemon frame that the query itself parses: the
    /// path is the same protobuf as for events, so the normalizer is also one —
    /// `RendererInfo::from_pb` (rule #12). The former seam carried the Python
    /// bridge's JSON maps here and required a second, "reverse" parser; it was
    /// removed together with the bridge.
    pub(crate) fn replace_all_frame(self: Pin<&mut Self>, frame: &[u8]) {
        let Ok(server_frame) = pb::ServerFrame::decode(frame) else {
            return;
        };
        let Some(pb::server_frame::Kind::Response(rsp)) = server_frame.kind else {
            return;
        };
        let Some(pb::response::Payload::RendererList(list)) = rsp.payload else {
            return;
        };
        self.replace_all(list.instances.iter().map(RendererInfo::from_pb).collect());
    }
}

// ============================================================================
// Display
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutInfo {
    pub fillmode: i32,
    pub align: i32,
    pub location_x: u32,
    pub location_y: u32,
    pub rotation: i32,
    /// waywallen 0.4.4: mirroring (`Flip`).
    pub flip: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutOverrideInfo {
    pub fillmode_set: bool,
    pub fillmode: i32,
    pub align_set: bool,
    pub align: i32,
    pub location_set: bool,
    pub location_x: u32,
    pub location_y: u32,
    pub rotation_set: bool,
    pub rotation: i32,
    /// waywallen 0.4.4: whether mirroring is set for the display, and the
    /// mirroring itself.
    pub flip_set: bool,
    pub flip: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkInfo {
    pub renderer_id: String,
    pub z_order: i32,
    /// The link is being shown right now (field 3 of `DisplayLinkInfo`). The
    /// canvas merges the links of its displays by it.
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistStatusInfo {
    pub active_id: i64,
    pub mode: i32,
    pub interval_secs: u32,
    pub current_id: String,
    pub position: u32,
    pub count: u32,
    pub remaining_secs: u32,
    /// waywallen 0.5.0: a smart playlist is waiting for suitable wallpapers
    /// (the selection is empty for now) — `PlaylistDisplayStatus.waiting`.
    pub waiting: bool,
    /// waywallen 0.5.0: the playlist error text on this display; empty means no
    /// error.
    pub error: String,
}

impl PlaylistStatusInfo {
    /// A port of `Display::playlistStatusFromPb`: `activeId <= 0` means there
    /// is no status.
    pub fn from_pb(s: &pb::PlaylistDisplayStatus) -> Option<Self> {
        (s.active_id > 0).then(|| PlaylistStatusInfo {
            active_id: s.active_id,
            mode: s.mode,
            interval_secs: s.interval_secs,
            current_id: s.current_id.clone(),
            position: s.position,
            count: s.count,
            remaining_secs: s.remaining_secs,
            waiting: s.waiting,
            error: s.error.clone(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayInfo {
    pub id: u64,
    pub name: String,
    pub alias: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub links: Vec<LinkInfo>,
    pub effective_layout: LayoutInfo,
    pub display_layout: LayoutInfo,
    pub layout_override: LayoutOverrideInfo,
    pub layout_overridden_by_wallpaper: bool,
    pub drm_render_major: u32,
    pub drm_render_minor: u32,
    /// The canvas the display is given to ("" — the display is on its own).
    /// Field 17 of `DisplayInfo` in the v0.3.5 schema.
    pub canvas_id: String,
    /// The display's place inside the canvas (field 18).
    pub canvas_rect: CanvasRect,
    /// How many canvas neighbours overlap it (field 19).
    pub canvas_overlap_count: u32,
    /// The display's key in the daemon settings (field 16).
    pub settings_key: String,
    /// What is going on with the display right now (field 14).
    pub conditions: Vec<core_weq::RuntimeCondition>,
    /// Manual pause of the display (field 21, waywallen 0.4.2).
    pub manual_paused: bool,
    /// Pause for any reason (field 22, waywallen 0.4.2).
    pub effective_paused: bool,
    /// Whether the display client supports window exclusions (field 23,
    /// waywallen 0.4.4): `WindowExclusionSupport` — 0 the old way, 1 supports,
    /// 2 partially.
    pub window_exclusion_support: i32,
    /// How many exclusion rules this display cannot apply (field 24, waywallen
    /// 0.4.4).
    pub unsupported_window_exclusions: u32,
}

/// Canvas rectangle. Our own copy of `pb::CanvasRect`, because prost
/// structs do not derive `Eq`, and the whole model differ rests on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CanvasRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl CanvasRect {
    fn from_pb(rect: Option<&pb::CanvasRect>) -> Self {
        rect.map(|r| Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        })
        .unwrap_or_default()
    }
}

fn layout_info(prefs: Option<&pb::LayoutPrefs>) -> LayoutInfo {
    let Some(p) = prefs else {
        return LayoutInfo::default();
    };
    LayoutInfo {
        fillmode: p.fillmode,
        align: p.align,
        location_x: p.location_x,
        location_y: p.location_y,
        rotation: p.rotation,
        flip: p.flip,
    }
}

fn layout_override_info(over: Option<&pb::LayoutOverride>) -> LayoutOverrideInfo {
    let Some(o) = over else {
        return LayoutOverrideInfo::default();
    };
    LayoutOverrideInfo {
        fillmode_set: o.fillmode_set,
        fillmode: o.fillmode,
        align_set: o.align_set,
        align: o.align,
        location_set: o.location_set,
        location_x: o.location_x,
        location_y: o.location_y,
        rotation_set: o.rotation_set,
        rotation: o.rotation,
        flip_set: o.flip_set,
        flip: o.flip,
    }
}

impl DisplayInfo {
    fn from_pb(info: &pb::DisplayInfo) -> Self {
        Self {
            id: info.display_id,
            name: info.name.clone(),
            alias: info.alias.clone(),
            width: info.width,
            height: info.height,
            refresh_mhz: info.refresh_mhz,
            links: info
                .links
                .iter()
                .map(|l| LinkInfo {
                    renderer_id: l.renderer_id.clone(),
                    z_order: l.z_order,
                    active: l.active,
                })
                .collect(),
            effective_layout: layout_info(info.effective_layout.as_ref()),
            display_layout: layout_info(info.display_layout.as_ref()),
            layout_override: layout_override_info(info.layout_override.as_ref()),
            layout_overridden_by_wallpaper: info.effective_layout_source
                == pb::LayoutSource::Wallpaper as i32,
            drm_render_major: info.drm_render_major,
            drm_render_minor: info.drm_render_minor,
            canvas_id: info.canvas_id.clone(),
            canvas_rect: CanvasRect::from_pb(info.canvas_rect.as_ref()),
            canvas_overlap_count: info.canvas_overlap_count,
            settings_key: info.settings_key.clone(),
            conditions: core_weq::runtime_conditions(&info.conditions),
            manual_paused: info.manual_paused,
            effective_paused: info.effective_paused,
            window_exclusion_support: info.window_exclusion_support,
            unsupported_window_exclusions: info.unsupported_window_exclusions,
        }
    }
}

#[derive(Default)]
pub struct DisplayRust {
    info: DisplayInfo,
    active_playlist_id: i64,
    playlist_status: Option<PlaylistStatusInfo>,
}

fn layout_map(layout: &LayoutInfo) -> qobject::QMap_QString_QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(QString::from("fillmode"), QVariant::from(&layout.fillmode));
    map.insert(QString::from("align"), QVariant::from(&layout.align));
    map.insert(
        QString::from("locationX"),
        QVariant::from(&(layout.location_x as i32)),
    );
    map.insert(
        QString::from("locationY"),
        QVariant::from(&(layout.location_y as i32)),
    );
    map.insert(QString::from("rotation"), QVariant::from(&layout.rotation));
    // waywallen 0.4.4: `flip` in the effective layout and in the display
    // layout.
    map.insert(QString::from("flip"), QVariant::from(&layout.flip));
    map
}

/// Links to renderers as a list of maps (`rendererId`/`zOrder`/`active`) —
/// one form for both a display and a canvas.
fn links_list(links: &[LinkInfo]) -> qobject::QList_QVariant {
    let mut out = QList::<QVariant>::default();
    for link in links {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        map.insert(
            QString::from("rendererId"),
            QVariant::from(&QString::from(&link.renderer_id)),
        );
        map.insert(QString::from("zOrder"), QVariant::from(&link.z_order));
        map.insert(QString::from("active"), QVariant::from(&link.active));
        out.append(QVariant::from(&map));
    }
    out
}

/// Playlist status as a map; without a playlist the map is empty. One form
/// for both a display and a canvas.
fn playlist_status_map(status: Option<&PlaylistStatusInfo>) -> qobject::QMap_QString_QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    let Some(status) = status else {
        return map;
    };
    map.insert(QString::from("activeId"), QVariant::from(&status.active_id));
    map.insert(QString::from("mode"), QVariant::from(&status.mode));
    map.insert(
        QString::from("intervalSecs"),
        QVariant::from(&(status.interval_secs as i32)),
    );
    map.insert(
        QString::from("currentId"),
        QVariant::from(&QString::from(&status.current_id)),
    );
    map.insert(
        QString::from("position"),
        QVariant::from(&(status.position as i32)),
    );
    map.insert(
        QString::from("count"),
        QVariant::from(&(status.count as i32)),
    );
    map.insert(
        QString::from("remainingSecs"),
        QVariant::from(&(status.remaining_secs as i32)),
    );
    // waywallen 0.5.0 (`display.cpp`): the keys are always there while there
    // is a status — `PlaylistListSheetState.qml` reads `status.error` and
    // `status.waiting`.
    map.insert(QString::from("waiting"), QVariant::from(&status.waiting));
    map.insert(
        QString::from("error"),
        QVariant::from(&QString::from(&status.error)),
    );
    map
}

/// Canvas rectangle as a map (`x`/`y`/`width`/`height`).
fn rect_map(r: CanvasRect) -> qobject::QMap_QString_QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(QString::from("x"), QVariant::from(&r.x));
    map.insert(QString::from("y"), QVariant::from(&r.y));
    map.insert(QString::from("width"), QVariant::from(&r.width));
    map.insert(QString::from("height"), QVariant::from(&r.height));
    map
}

impl qobject::Display {
    fn display_id(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.rust().info.id)
    }
    fn display_name(&self) -> QString {
        QString::from(&self.rust().info.name)
    }
    fn alias(&self) -> QString {
        QString::from(&self.rust().info.alias)
    }
    fn display_label(&self) -> QString {
        let info = &self.rust().info;
        QString::from(&rexon_core::we_util::display_label(&info.alias, &info.name, info.id))
    }
    fn width(&self) -> u32 {
        self.rust().info.width
    }
    fn height(&self) -> u32 {
        self.rust().info.height
    }
    fn refresh_mhz(&self) -> u32 {
        self.rust().info.refresh_mhz
    }

    fn links(&self) -> qobject::QList_QVariant {
        links_list(&self.rust().info.links)
    }

    fn effective_layout(&self) -> qobject::QMap_QString_QVariant {
        layout_map(&self.rust().info.effective_layout)
    }

    fn display_layout(&self) -> qobject::QMap_QString_QVariant {
        layout_map(&self.rust().info.display_layout)
    }

    fn layout_override(&self) -> qobject::QMap_QString_QVariant {
        let over = &self.rust().info.layout_override;
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        map.insert(
            QString::from("fillmodeSet"),
            QVariant::from(&over.fillmode_set),
        );
        map.insert(QString::from("fillmode"), QVariant::from(&over.fillmode));
        map.insert(QString::from("alignSet"), QVariant::from(&over.align_set));
        map.insert(QString::from("align"), QVariant::from(&over.align));
        map.insert(
            QString::from("locationSet"),
            QVariant::from(&over.location_set),
        );
        map.insert(
            QString::from("locationX"),
            QVariant::from(&(over.location_x as i32)),
        );
        map.insert(
            QString::from("locationY"),
            QVariant::from(&(over.location_y as i32)),
        );
        map.insert(
            QString::from("rotationSet"),
            QVariant::from(&over.rotation_set),
        );
        map.insert(QString::from("rotation"), QVariant::from(&over.rotation));
        // waywallen 0.4.4: mirroring in the display override.
        map.insert(QString::from("flipSet"), QVariant::from(&over.flip_set));
        map.insert(QString::from("flip"), QVariant::from(&over.flip));
        map
    }

    fn layout_overridden_by_wallpaper(&self) -> bool {
        self.rust().info.layout_overridden_by_wallpaper
    }
    fn drm_render_major(&self) -> u32 {
        self.rust().info.drm_render_major
    }
    fn drm_render_minor(&self) -> u32 {
        self.rust().info.drm_render_minor
    }

    /// The daemon's formula one to one: the target is selectable while the
    /// display is not given to a canvas
    /// (`selectable_target = canvas_id.is_none()`,
    /// `src/wallframe/routing/router/mod.rs:2666` of the vendor at v0.3.5).
    fn selectable_target(&self) -> bool {
        self.rust().info.canvas_id.is_empty()
    }

    fn canvas_id(&self) -> QString {
        QString::from(&self.rust().info.canvas_id)
    }

    /// Canvas rectangle as a map — the same shape as the vendor's
    /// `scenePosition` (`DisplaysPage.qml` reads `x`/`y`/`width`/`height`).
    /// Without a canvas the map is zero rather than missing: QML reads the
    /// fields unconditionally.
    fn canvas_rect(&self) -> qobject::QMap_QString_QVariant {
        rect_map(self.rust().info.canvas_rect)
    }

    fn canvas_overlap_count(&self) -> u32 {
        self.rust().info.canvas_overlap_count
    }

    fn manual_paused(&self) -> bool {
        self.rust().info.manual_paused
    }

    fn effective_paused(&self) -> bool {
        self.rust().info.effective_paused
    }

    fn settings_key(&self) -> QString {
        QString::from(&self.rust().info.settings_key)
    }

    fn runtime_conditions(&self) -> qobject::QList_QVariant {
        conditions_list(&self.rust().info.conditions)
    }

    fn window_exclusion_support(&self) -> i32 {
        self.rust().info.window_exclusion_support
    }

    fn unsupported_window_exclusions(&self) -> u32 {
        self.rust().info.unsupported_window_exclusions
    }

    fn active_playlist_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.rust().active_playlist_id)
    }

    fn playlist_status(&self) -> qobject::QMap_QString_QVariant {
        playlist_status_map(self.rust().playlist_status.as_ref())
    }

    fn install(mut self: Pin<&mut Self>, info: DisplayInfo) {
        self.as_mut().rust_mut().info = info;
    }

    /// A port of `Display::updateFrom` — one signal per changed field.
    fn update_from(mut self: Pin<&mut Self>, info: DisplayInfo) {
        let old = self.rust().info.clone();
        self.as_mut().rust_mut().info = info;
        let new = self.rust().info.clone();
        // waywallen 0.4.4: one `windowExclusionSupportChanged` for both fields,
        // and first — as in the original's `Display::updateFrom`.
        if old.window_exclusion_support != new.window_exclusion_support
            || old.unsupported_window_exclusions != new.unsupported_window_exclusions
        {
            self.as_mut().window_exclusion_support_changed();
        }
        // The original (v0.4.2) sends one `pauseChanged` for both flags.
        if old.manual_paused != new.manual_paused || old.effective_paused != new.effective_paused {
            self.as_mut().pause_changed();
        }
        if old.name != new.name {
            self.as_mut().name_changed();
        }
        if old.alias != new.alias {
            self.as_mut().alias_changed();
        }
        if old.name != new.name || old.alias != new.alias {
            self.as_mut().display_label_changed();
        }
        if old.width != new.width || old.height != new.height {
            self.as_mut().size_changed();
        }
        if old.refresh_mhz != new.refresh_mhz {
            self.as_mut().refresh_mhz_changed();
        }
        if old.links != new.links {
            self.as_mut().links_changed();
        }
        if old.effective_layout != new.effective_layout
            || old.display_layout != new.display_layout
            || old.layout_override != new.layout_override
            || old.layout_overridden_by_wallpaper != new.layout_overridden_by_wallpaper
        {
            self.as_mut().layout_changed();
        }
        if old.canvas_id != new.canvas_id
            || old.canvas_rect != new.canvas_rect
            || old.canvas_overlap_count != new.canvas_overlap_count
        {
            self.as_mut().canvas_changed();
        }
        if old.settings_key != new.settings_key {
            self.as_mut().settings_key_changed();
        }
        if old.conditions != new.conditions {
            self.as_mut().runtime_conditions_changed();
        }
    }

    /// A port of `Display::updatePlaylistStatus`: `activeId <= 0` means the
    /// status is empty.
    fn update_playlist_status(mut self: Pin<&mut Self>, status: Option<&pb::PlaylistDisplayStatus>) {
        let (active, snapshot) = match status {
            Some(s) => (s.active_id, PlaylistStatusInfo::from_pb(s)),
            None => (0, None),
        };
        if self.rust().active_playlist_id == active && self.rust().playlist_status == snapshot {
            return;
        }
        self.as_mut().rust_mut().active_playlist_id = active;
        self.as_mut().rust_mut().playlist_status = snapshot;
        self.as_mut().playlist_status_changed();
    }
}

// ============================================================================
// Canvas
// ============================================================================

/// One canvas member — a mirror of `proto::CanvasMemberInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasMemberInfo {
    /// The display's key in the daemon settings: a canvas member is a place for
    /// a display, not the display itself, and it lives even without a connected
    /// screen.
    pub settings_key: String,
    pub rect: Option<CanvasRect>,
    /// The largest live surface size of the member (width, height); if the
    /// member is offline, the daemon does not send the field.
    pub minimum_scale_to: Option<(u32, u32)>,
    pub aspect_locked: bool,
    /// The displays of this member connected right now.
    pub display_ids: Vec<u64>,
}

/// What the daemon sends about the canvas (`proto::CanvasInfo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasInfo {
    pub id: String,
    pub name: String,
    pub members: Vec<CanvasMemberInfo>,
    pub extent: Option<CanvasRect>,
    pub layout_override: Option<LayoutOverrideInfo>,
    pub effective_layout: Option<LayoutInfo>,
    pub wallpaper_id: String,
    pub revision: u64,
}

impl CanvasInfo {
    fn from_pb(info: &pb::CanvasInfo) -> Self {
        Self {
            id: info.canvas_id.clone(),
            name: info.name.clone(),
            members: info
                .members
                .iter()
                .map(|m| CanvasMemberInfo {
                    settings_key: m.settings_key.clone(),
                    rect: m.rect.as_ref().map(|r| CanvasRect::from_pb(Some(r))),
                    minimum_scale_to: m.minimum_scale_to.as_ref().map(|s| (s.width, s.height)),
                    aspect_locked: m.aspect_locked,
                    display_ids: m.display_ids.clone(),
                })
                .collect(),
            extent: info.extent.as_ref().map(|r| CanvasRect::from_pb(Some(r))),
            layout_override: info
                .layout_override
                .as_ref()
                .map(|o| layout_override_info(Some(o))),
            effective_layout: info
                .effective_layout
                .as_ref()
                .map(|l| layout_info(Some(l))),
            wallpaper_id: info.wallpaper_id.clone(),
            revision: info.revision,
        }
    }

    /// How many displays of the canvas are connected right now (over all
    /// members).
    fn online_count(&self) -> usize {
        self.members.iter().map(|m| m.display_ids.len()).sum()
    }
}

/// Assembled from the canvas displays: the daemon does not send this about
/// the canvas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasRuntime {
    pub links: Vec<LinkInfo>,
    pub active_playlist_id: i64,
    pub playlist_status: Option<PlaylistStatusInfo>,
    pub conditions: Vec<core_weq::RuntimeCondition>,
}

/// What the canvas needs to know about a display.
pub struct CanvasDisplayView<'a> {
    pub canvas_id: &'a str,
    pub links: &'a [LinkInfo],
    pub conditions: &'a [core_weq::RuntimeCondition],
    pub active_playlist_id: i64,
    pub playlist_status: Option<&'a PlaylistStatusInfo>,
}

/// A port of `Canvas::updateRuntime`. The displays go in ascending id
/// order.
///
/// * links are merged by renderer: the first one met stays, and `active` is
///   raised if the link is shown on at least one display of the canvas;
///   the result is ordered by renderer identifier;
/// * conditions — without repeats, in order of appearance;
/// * the playlist — of the first canvas display that has one.
pub fn canvas_runtime(canvas_id: &str, displays: &[CanvasDisplayView<'_>]) -> CanvasRuntime {
    let mut links: BTreeMap<&str, LinkInfo> = BTreeMap::new();
    let mut runtime = CanvasRuntime::default();
    for display in displays.iter().filter(|d| d.canvas_id == canvas_id) {
        for link in display.links {
            match links.get_mut(link.renderer_id.as_str()) {
                Some(seen) => seen.active |= link.active,
                None => {
                    links.insert(link.renderer_id.as_str(), link.clone());
                }
            }
        }
        for condition in display.conditions {
            if !runtime.conditions.contains(condition) {
                runtime.conditions.push(condition.clone());
            }
        }
        if runtime.active_playlist_id == 0 && display.active_playlist_id > 0 {
            runtime.active_playlist_id = display.active_playlist_id;
            runtime.playlist_status = display.playlist_status.cloned();
        }
    }
    runtime.links = links.into_values().collect();
    runtime
}

#[derive(Default)]
pub struct CanvasRust {
    info: CanvasInfo,
    runtime: CanvasRuntime,
}

impl qobject::Canvas {
    fn canvas_object_id(&self) -> QString {
        QString::from(&self.rust().info.id)
    }
    fn canvas_name(&self) -> QString {
        QString::from(&self.rust().info.name)
    }

    fn members(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        for member in &self.rust().info.members {
            let mut row = QMap::<QMapPair_QString_QVariant>::default();
            row.insert(
                QString::from("settingsKey"),
                QVariant::from(&QString::from(&member.settings_key)),
            );
            // If there is no rectangle or size, the map is empty rather than missing:
            // the markup reads the row's fields unconditionally.
            let rect = member.rect.map(rect_map).unwrap_or_default();
            row.insert(QString::from("rect"), QVariant::from(&rect));
            let mut minimum = QMap::<QMapPair_QString_QVariant>::default();
            if let Some((width, height)) = member.minimum_scale_to {
                minimum.insert(QString::from("width"), QVariant::from(&width));
                minimum.insert(QString::from("height"), QVariant::from(&height));
            }
            row.insert(QString::from("minimumScaleTo"), QVariant::from(&minimum));
            row.insert(
                QString::from("aspectLocked"),
                QVariant::from(&member.aspect_locked),
            );
            // Warning: display numbers are `f64`, like `Display::id`: we do not expose
            // a 64-bit integer to QML, and the markup compares them with `display.id`.
            let mut ids = QList::<QVariant>::default();
            for id in &member.display_ids {
                ids.append(QVariant::from(&crate::qvalue::u64_to_qml(*id)));
            }
            row.insert(QString::from("displayIds"), QVariant::from(&ids));
            let online = member.display_ids.len();
            row.insert(QString::from("onlineCount"), QVariant::from(&(online as i32)));
            // Two live displays in the same place of the canvas.
            row.insert(QString::from("overlap"), QVariant::from(&(online > 1)));
            out.append(QVariant::from(&row));
        }
        out
    }

    fn extent(&self) -> qobject::QMap_QString_QVariant {
        self.rust().info.extent.map(rect_map).unwrap_or_default()
    }
    fn extent_width(&self) -> u32 {
        self.rust().info.extent.map(|r| r.width).unwrap_or(0)
    }
    fn extent_height(&self) -> u32 {
        self.rust().info.extent.map(|r| r.height).unwrap_or(0)
    }

    /// Canvas layout override. A canvas has no alignment (`align`) — only the
    /// keys that `CanvasLayoutSetQuery` sets.
    fn canvas_layout_override(&self) -> qobject::QMap_QString_QVariant {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        let Some(over) = self.rust().info.layout_override.as_ref() else {
            return map;
        };
        map.insert(
            QString::from("fillmodeSet"),
            QVariant::from(&over.fillmode_set),
        );
        map.insert(QString::from("fillmode"), QVariant::from(&over.fillmode));
        map.insert(
            QString::from("locationSet"),
            QVariant::from(&over.location_set),
        );
        map.insert(
            QString::from("locationX"),
            QVariant::from(&(over.location_x as i32)),
        );
        map.insert(
            QString::from("locationY"),
            QVariant::from(&(over.location_y as i32)),
        );
        map.insert(
            QString::from("rotationSet"),
            QVariant::from(&over.rotation_set),
        );
        map.insert(QString::from("rotation"), QVariant::from(&over.rotation));
        map.insert(QString::from("flipSet"), QVariant::from(&over.flip_set));
        map.insert(QString::from("flip"), QVariant::from(&over.flip));
        map
    }

    fn canvas_effective_layout(&self) -> qobject::QMap_QString_QVariant {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        let Some(layout) = self.rust().info.effective_layout.as_ref() else {
            return map;
        };
        map.insert(QString::from("fillmode"), QVariant::from(&layout.fillmode));
        map.insert(
            QString::from("locationX"),
            QVariant::from(&(layout.location_x as i32)),
        );
        map.insert(
            QString::from("locationY"),
            QVariant::from(&(layout.location_y as i32)),
        );
        map.insert(QString::from("rotation"), QVariant::from(&layout.rotation));
        map.insert(QString::from("flip"), QVariant::from(&layout.flip));
        map
    }

    fn canvas_links(&self) -> qobject::QList_QVariant {
        links_list(&self.rust().runtime.links)
    }
    fn canvas_active_playlist_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.rust().runtime.active_playlist_id)
    }
    fn canvas_playlist_status(&self) -> qobject::QMap_QString_QVariant {
        playlist_status_map(self.rust().runtime.playlist_status.as_ref())
    }
    fn canvas_runtime_conditions(&self) -> qobject::QList_QVariant {
        conditions_list(&self.rust().runtime.conditions)
    }
    fn wallpaper_id(&self) -> QString {
        QString::from(&self.rust().info.wallpaper_id)
    }
    fn revision(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.rust().info.revision)
    }
    fn member_count(&self) -> i32 {
        self.rust().info.members.len() as i32
    }
    fn online_count(&self) -> i32 {
        self.rust().info.online_count() as i32
    }
    fn has_live_displays(&self) -> bool {
        self.rust().info.online_count() > 0
    }
    fn empty(&self) -> bool {
        self.rust().info.members.is_empty()
    }

    /// Initial filling: we send no signals, the object has not been handed to
    /// anyone yet.
    fn install(mut self: Pin<&mut Self>, info: CanvasInfo) {
        self.as_mut().rust_mut().info = info;
    }

    /// A port of `Canvas::updateFrom`: one `changed` for any difference in the
    /// snapshot.
    fn update_from(mut self: Pin<&mut Self>, info: CanvasInfo) {
        if self.rust().info == info {
            return;
        }
        self.as_mut().rust_mut().info = info;
        self.as_mut().changed();
    }

    /// Accept what was recomputed from the displays; the signal only on a
    /// difference.
    fn set_runtime(mut self: Pin<&mut Self>, runtime: CanvasRuntime) {
        if self.rust().runtime == runtime {
            return;
        }
        self.as_mut().rust_mut().runtime = runtime;
        self.as_mut().runtime_changed();
    }
}

// ============================================================================
// DisplayManager
// ============================================================================

#[derive(Default)]
pub struct DisplayManagerRust {
    by_id: BTreeMap<u64, *mut qobject::QObject>,
    canvas_by_id: BTreeMap<String, *mut qobject::QObject>,
    /// Canvases in display order: by name, then by identifier.
    canvas_order: Vec<*mut qobject::QObject>,
    canvas_revision: u64,
}

/// # Safety
/// `ptr` is a live `Display` created by this bridge's factory.
unsafe fn display_pin<'a>(ptr: *mut qobject::QObject) -> Pin<&'a mut qobject::Display> {
    unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Display>()) }
}

/// # Safety
/// `ptr` is a live `Canvas` created by this bridge's factory.
unsafe fn canvas_pin<'a>(ptr: *mut qobject::QObject) -> Pin<&'a mut qobject::Canvas> {
    unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Canvas>()) }
}

impl qobject::DisplayManager {
    fn displays(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        for ptr in self.rust().by_id.values() {
            out.append(unsafe { qobject::rexon_object_to_variant(*ptr) });
        }
        out
    }

    fn canvases(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        for ptr in &self.rust().canvas_order {
            out.append(unsafe { qobject::rexon_object_to_variant(*ptr) });
        }
        out
    }

    fn canvas_revision(&self) -> f64 {
        crate::qvalue::u64_to_qml(self.rust().canvas_revision)
    }

    fn count(&self) -> i32 {
        self.rust().by_id.len() as i32
    }

    fn has_active_playlist_displays(&self) -> bool {
        self.rust().by_id.values().any(|ptr| {
            let display: &qobject::Display = unsafe { &*ptr.cast::<qobject::Display>() };
            display.rust().active_playlist_id > 0
        })
    }

    /// Display label by identifier — the same as in `displayLabel`.
    pub(crate) fn display_label_of(&self, id: u64) -> Option<String> {
        self.rust().by_id.get(&id).map(|ptr| {
            let display: &qobject::Display = unsafe { &*ptr.cast::<qobject::Display>() };
            let info = &display.rust().info;
            rexon_core::we_util::display_label(&info.alias, &info.name, info.id)
        })
    }

    /// Canvas name by identifier.
    pub(crate) fn canvas_name_of(&self, id: &str) -> Option<String> {
        self.rust().canvas_by_id.get(id).map(|ptr| {
            let canvas: &qobject::Canvas = unsafe { &*ptr.cast::<qobject::Canvas>() };
            canvas.rust().info.name.clone()
        })
    }

    fn get(&self, id: f64) -> *mut qobject::QObject {
        let id = crate::qvalue::qml_to_u64(id);
        self.rust()
            .by_id
            .get(&id)
            .copied()
            .unwrap_or(std::ptr::null_mut())
    }

    fn get_canvas(&self, id: &QString) -> *mut qobject::QObject {
        self.rust()
            .canvas_by_id
            .get(&id.to_string())
            .copied()
            .unwrap_or(std::ptr::null_mut())
    }

    /// A port of `DisplayManager::replaceCanvases`: a snapshot is a full
    /// replacement. Known canvases are updated in place (the same object — the
    /// page's bindings hang on it), vanished ones are removed, and
    /// `canvasesChanged` is sent for every snapshot.
    fn replace_canvases(mut self: Pin<&mut Self>, infos: Vec<CanvasInfo>, revision: u64) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::DisplayManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::DisplayManager).cast()
        };
        let mut old = std::mem::take(&mut self.as_mut().rust_mut().canvas_by_id);
        let mut next: BTreeMap<String, *mut qobject::QObject> = BTreeMap::new();
        // Order key: (name, identifier).
        let mut order: Vec<((String, String), *mut qobject::QObject)> = Vec::new();
        for info in infos {
            let key = (info.name.clone(), info.id.clone());
            // A repeated identifier in one snapshot: the object is already in `next`.
            let ptr = match old.remove(&info.id).or_else(|| next.get(&info.id).copied()) {
                Some(ptr) => {
                    unsafe { canvas_pin(ptr) }.update_from(info);
                    ptr
                }
                None => {
                    let ptr = unsafe { qobject::rexon_new_canvas(parent) };
                    unsafe { canvas_pin(ptr) }.install(info);
                    ptr
                }
            };
            if next.insert(key.1.clone(), ptr).is_some() {
                order.retain(|(_, seen)| *seen != ptr);
            }
            order.push((key, ptr));
        }
        for ptr in old.into_values() {
            unsafe { qobject::rexon_delete_child_later(ptr) };
        }
        order.sort_by(|a, b| a.0.cmp(&b.0));
        {
            let mut rust = self.as_mut().rust_mut();
            rust.canvas_by_id = next;
            rust.canvas_order = order.into_iter().map(|(_, ptr)| ptr).collect();
            rust.canvas_revision = revision;
        }
        self.as_mut().refresh_canvas_runtime();
        self.as_mut().canvases_changed();
    }

    /// A port of `DisplayManager::refreshCanvasRuntime`: reassemble the links,
    /// playlist and conditions of every canvas from its displays. Call after
    /// ANY change of displays, playlists or the canvases themselves.
    fn refresh_canvas_runtime(self: Pin<&mut Self>) {
        if self.rust().canvas_order.is_empty() {
            return;
        }
        let displays: Vec<&qobject::Display> = self
            .rust()
            .by_id
            .values()
            .map(|ptr| unsafe { &*ptr.cast::<qobject::Display>() })
            .collect();
        let views: Vec<CanvasDisplayView<'_>> = displays
            .iter()
            .map(|display| {
                let rust = display.rust();
                CanvasDisplayView {
                    canvas_id: &rust.info.canvas_id,
                    links: &rust.info.links,
                    conditions: &rust.info.conditions,
                    active_playlist_id: rust.active_playlist_id,
                    playlist_status: rust.playlist_status.as_ref(),
                }
            })
            .collect();
        for ptr in self.rust().canvas_order.clone() {
            let id = unsafe { &*ptr.cast::<qobject::Canvas>() }.rust().info.id.clone();
            unsafe { canvas_pin(ptr) }.set_runtime(canvas_runtime(&id, &views));
        }
    }

    fn replace_all(mut self: Pin<&mut Self>, infos: Vec<DisplayInfo>) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::DisplayManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::DisplayManager).cast()
        };
        let fresh: BTreeMap<u64, DisplayInfo> = infos.into_iter().map(|i| (i.id, i)).collect();
        let had_active = self.has_active_playlist_displays();

        let stale: Vec<u64> = self
            .rust()
            .by_id
            .keys()
            .filter(|id| !fresh.contains_key(*id))
            .copied()
            .collect();
        let mut changed = !stale.is_empty();
        for id in stale {
            if let Some(ptr) = self.as_mut().rust_mut().by_id.remove(&id) {
                unsafe { qobject::rexon_delete_child_later(ptr) };
            }
        }

        for (id, info) in fresh {
            let existing = self.rust().by_id.get(&id).copied();
            match existing {
                Some(ptr) => unsafe { display_pin(ptr) }.update_from(info),
                None => {
                    let ptr = unsafe { qobject::rexon_new_display(parent) };
                    unsafe { display_pin(ptr) }.install(info);
                    self.as_mut().rust_mut().by_id.insert(id, ptr);
                    changed = true;
                }
            }
        }

        self.as_mut().refresh_canvas_runtime();
        // A display with a playlist left — the "there are active ones" flag may
        // have changed.
        if had_active != self.has_active_playlist_displays() {
            self.as_mut().playlist_status_changed();
        }
        if changed {
            self.as_mut().displays_changed();
        }
    }

    fn upsert(mut self: Pin<&mut Self>, info: DisplayInfo) {
        if let Some(ptr) = self.rust().by_id.get(&info.id).copied() {
            unsafe { display_pin(ptr) }.update_from(info);
            self.as_mut().refresh_canvas_runtime();
            return;
        }
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::DisplayManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::DisplayManager).cast()
        };
        let id = info.id;
        let ptr = unsafe { qobject::rexon_new_display(parent) };
        unsafe { display_pin(ptr) }.install(info);
        self.as_mut().rust_mut().by_id.insert(id, ptr);
        self.as_mut().refresh_canvas_runtime();
        self.as_mut().displays_changed();
    }

    fn remove(mut self: Pin<&mut Self>, id: u64) {
        let had_active = self.has_active_playlist_displays();
        let Some(ptr) = self.as_mut().rust_mut().by_id.remove(&id) else {
            return;
        };
        unsafe { qobject::rexon_delete_child_later(ptr) };
        self.as_mut().refresh_canvas_runtime();
        if had_active != self.has_active_playlist_displays() {
            self.as_mut().playlist_status_changed();
        }
        self.as_mut().displays_changed();
    }

    /// A port of `DisplayManager::replacePlaylistStatuses`: displays outside
    /// the list get an empty status (otherwise a dead playlist would hang on
    /// the screen).
    fn replace_playlist_statuses(mut self: Pin<&mut Self>, list: &[pb::PlaylistDisplayStatus]) {
        let had_active = self.has_active_playlist_displays();
        let by_id: BTreeMap<u64, &pb::PlaylistDisplayStatus> =
            list.iter().map(|s| (s.display_id, s)).collect();
        let pointers: Vec<*mut qobject::QObject> = self.rust().by_id.values().copied().collect();
        let ids: Vec<u64> = self.rust().by_id.keys().copied().collect();
        for (id, ptr) in ids.iter().zip(pointers) {
            unsafe { display_pin(ptr) }.update_playlist_status(by_id.get(id).copied());
        }
        self.as_mut().refresh_canvas_runtime();
        if had_active != self.has_active_playlist_displays() {
            self.as_mut().playlist_status_changed();
        }
    }

    pub(crate) fn handle_event(self: Pin<&mut Self>, event: &pb::Event) {
        use pb::event::Payload;
        match event.payload.as_ref() {
            Some(Payload::DisplaySnapshot(snap)) => {
                self.replace_all(snap.displays.iter().map(DisplayInfo::from_pb).collect());
            }
            Some(Payload::DisplayChanged(changed)) => {
                if let Some(info) = changed.display.as_ref() {
                    self.upsert(DisplayInfo::from_pb(info));
                }
            }
            Some(Payload::DisplayRemoved(removed)) => {
                self.remove(removed.display_id);
            }
            Some(Payload::PlaylistChanged(changed)) => {
                self.replace_playlist_statuses(&changed.displays);
            }
            Some(Payload::CanvasSnapshot(snap)) => {
                self.replace_canvases(
                    snap.canvases.iter().map(CanvasInfo::from_pb).collect(),
                    snap.revision,
                );
            }
            _ => {}
        }
    }
}

// ============================================================================
// Gpu / GpuManager
// ============================================================================

#[derive(Default)]
pub struct GpuRust {
    info: pb::GpuInfo,
}

impl qobject::Gpu {
    fn render_node(&self) -> QString {
        QString::from(&self.rust().info.render_node)
    }
    fn primary_node(&self) -> QString {
        QString::from(&self.rust().info.primary_node)
    }
    fn render_major(&self) -> u32 {
        self.rust().info.render_major
    }
    fn render_minor(&self) -> u32 {
        self.rust().info.render_minor
    }
    fn primary_major(&self) -> u32 {
        self.rust().info.primary_major
    }
    fn primary_minor(&self) -> u32 {
        self.rust().info.primary_minor
    }
    fn pci_bdf(&self) -> QString {
        QString::from(&self.rust().info.pci_bdf)
    }
    fn vendor_id(&self) -> u32 {
        self.rust().info.vendor_id
    }
    fn device_id(&self) -> u32 {
        self.rust().info.device_id
    }
    fn driver(&self) -> QString {
        QString::from(&self.rust().info.driver)
    }
    fn gpu_name(&self) -> QString {
        QString::from(&self.rust().info.name)
    }
    fn description(&self) -> QString {
        QString::from(&self.rust().info.description)
    }

    /// Fill in before the object is handed to anyone: the properties are
    /// constant, they have no signals.
    fn install(mut self: Pin<&mut Self>, info: pb::GpuInfo) {
        self.as_mut().rust_mut().info = info;
    }
}

/// The order is as the daemon sent it. `infos` and `objects` go row for
/// row: the former are data for lookup and comparison, the latter are the
/// objects Qt owns (they are children of the manager).
#[derive(Default)]
pub struct GpuManagerRust {
    infos: Vec<pb::GpuInfo>,
    objects: Vec<*mut qobject::QObject>,
}

impl qobject::GpuManager {
    fn gpus(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        for ptr in &self.rust().objects {
            out.append(unsafe { qobject::rexon_object_to_variant(*ptr) });
        }
        out
    }

    fn count(&self) -> i32 {
        self.rust().objects.len() as i32
    }

    fn find(&self, major: u32, minor: u32) -> *mut qobject::QObject {
        let rust = self.rust();
        core_weq::find_gpu(&rust.infos, major, minor)
            .and_then(|index| rust.objects.get(index).copied())
            .unwrap_or(std::ptr::null_mut())
    }

    /// A full replacement of the list with the `gpu_list` answer.
    ///
    /// Warning: the same list a second time (reconnecting to the same daemon)
    /// does NOT recreate the objects. The card icon finds its object once, with
    /// a `find` call, and does not watch `gpusChanged`: if we removed the
    /// objects, the icons at renderers and displays would disappear until the
    /// page is recreated, although the cards are the same.
    ///
    /// Returns whether the list changed: by this flag App tells the icons to
    /// find their cards anew (`gpuManagerChanged`).
    pub(crate) fn replace_all(mut self: Pin<&mut Self>, infos: Vec<pb::GpuInfo>) -> bool {
        if self.rust().infos == infos {
            return false;
        }
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::GpuManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::GpuManager).cast()
        };
        let stale = std::mem::take(&mut self.as_mut().rust_mut().objects);
        for ptr in stale {
            unsafe { qobject::rexon_delete_child_later(ptr) };
        }
        let mut objects = Vec::with_capacity(infos.len());
        for info in &infos {
            let ptr = unsafe { qobject::rexon_new_gpu(parent) };
            unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Gpu>()) }.install(info.clone());
            objects.push(ptr);
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.infos = infos;
            rust.objects = objects;
        }
        self.as_mut().gpus_changed();
        true
    }
}

// ============================================================================
// NowPlayingModel / PresentationManager
// ============================================================================

/// A row of the "Current wallpapers" card.
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlayingRow {
    pub wallpaper_id: String,
    /// The wallpaper in QML terms (`we_wallpaper::entry_to_map`).
    pub wallpaper: serde_json::Map<String, serde_json::Value>,
    pub target_summary: String,
    pub state: i32,
}

/// Role names — character for character the ones `SidebarNowPlaying.qml`
/// reads.
const NOW_PLAYING_ROLES: [&str; 4] = ["wallpaper", "wallpaperId", "targetSummary", "state"];
/// `Qt::UserRole`.
const NOW_PLAYING_ROLE_BASE: i32 = 0x0100;

#[derive(Default)]
pub struct NowPlayingModelRust {
    rows: Vec<NowPlayingRow>,
}

fn now_playing_role(row: &NowPlayingRow, name: &str) -> QVariant {
    match name {
        "wallpaper" => {
            crate::qvalue::json_to_variant(&serde_json::Value::Object(row.wallpaper.clone()))
        }
        "wallpaperId" => QVariant::from(&QString::from(&row.wallpaper_id)),
        "targetSummary" => QVariant::from(&QString::from(&row.target_summary)),
        "state" => QVariant::from(&row.state),
        _ => QVariant::default(),
    }
}

impl qobject::NowPlayingModel {
    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    fn row_count(&self, parent: &qobject::QModelIndex) -> i32 {
        // In a flat list only the root has children.
        if parent.is_valid() {
            0
        } else {
            self.count()
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (slot, name) in NOW_PLAYING_ROLES.iter().enumerate() {
            names.insert(
                NOW_PLAYING_ROLE_BASE + slot as i32,
                cxx_qt_lib::QByteArray::from(*name),
            );
        }
        names
    }

    fn data(&self, index: &qobject::QModelIndex, role: i32) -> QVariant {
        let name = usize::try_from(role - NOW_PLAYING_ROLE_BASE)
            .ok()
            .and_then(|slot| NOW_PLAYING_ROLES.get(slot));
        let row = usize::try_from(index.row())
            .ok()
            .filter(|_| index.is_valid())
            .and_then(|row| self.rust().rows.get(row));
        match (row, name) {
            (Some(row), Some(name)) => now_playing_role(row, name),
            _ => QVariant::default(),
        }
    }

    fn item(&self, row: i32) -> qobject::QMap_QString_QVariant {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        let Some(row) = usize::try_from(row).ok().and_then(|row| self.rust().rows.get(row)) else {
            return out;
        };
        for name in NOW_PLAYING_ROLES {
            out.insert(QString::from(name), now_playing_role(row, name));
        }
        out
    }

    /// Replace the rows. The same wallpapers in the same order (the wallpaper
    /// data arrived or the target label changed) are updated in place — the
    /// card is not rebuilt and does not lose its position; a different
    /// composition means a model reset.
    fn replace(mut self: Pin<&mut Self>, rows: Vec<NowPlayingRow>) {
        if self.rust().rows == rows {
            return;
        }
        let before = self.rust().rows.len();
        let same_order = before == rows.len()
            && self
                .rust()
                .rows
                .iter()
                .zip(&rows)
                .all(|(old, new)| old.wallpaper_id == new.wallpaper_id);
        if same_order {
            self.as_mut().rust_mut().rows = rows;
            let root = qobject::QModelIndex::default();
            let first = self.index(0, 0, &root);
            let last = self.index(before as i32 - 1, 0, &root);
            self.as_mut()
                .data_changed(&first, &last, &cxx_qt_lib::QVector::<i32>::default());
            return;
        }
        let after = rows.len();
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().rows = rows;
            self.as_mut().end_reset_model();
        }
        if before != after {
            self.as_mut().count_changed();
        }
    }
}

/// The daemon's display snapshot and the wallpapers already known for it.
pub struct PresentationManagerRust {
    presentations: Vec<pb::WallpaperPresentationInfo>,
    /// Wallpapers by identifier. They survive a disconnection: after
    /// reconnecting the card immediately shows the name and preview, and the
    /// daemon's fresh answer updates them.
    wallpapers: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    model: *mut qobject::QObject,
}

impl Default for PresentationManagerRust {
    fn default() -> Self {
        Self {
            presentations: Vec::new(),
            wallpapers: BTreeMap::new(),
            model: std::ptr::null_mut(),
        }
    }
}

/// A vendor translation string from the display manager's context with a
/// substitution.
fn presentation_fallback_label(source: &str, id: &str) -> String {
    let template = qobject::rexon_we_vendor_tr(
        &QString::from("waywallen::PresentationManager"),
        &QString::from(source),
    )
    .to_string();
    rexon_core::i18n::fill(&template, &[&id])
}

impl qobject::PresentationManager {
    fn count(&self) -> i32 {
        self.rust().presentations.len() as i32
    }

    fn model(&self) -> *mut qobject::QObject {
        self.rust().model
    }

    /// Create the row model. App calls it right after creating the manager —
    /// before the markup reads the `model` property.
    pub(crate) fn attach(mut self: Pin<&mut Self>) {
        if !self.rust().model.is_null() {
            return;
        }
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::PresentationManager =
                unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::PresentationManager).cast()
        };
        let model = unsafe { qobject::rexon_new_now_playing_model(parent) };
        self.as_mut().rust_mut().model = model;
    }

    pub(crate) fn has_presentations(&self) -> bool {
        !self.rust().presentations.is_empty()
    }

    /// A new display snapshot. Returns the identifiers of the wallpapers to ask
    /// the daemon about (all from the snapshot: known ones may have changed).
    pub(crate) fn replace_snapshot(
        mut self: Pin<&mut Self>,
        presentations: Vec<pb::WallpaperPresentationInfo>,
        displays: Option<&qobject::DisplayManager>,
    ) -> Vec<String> {
        let ids = core_weq::presentation_wallpaper_ids(&presentations);
        self.as_mut().set_presentations(presentations, displays);
        ids
    }

    /// The connection to the daemon is lost — there are no displays until a
    /// new snapshot arrives.
    pub(crate) fn clear(self: Pin<&mut Self>) {
        self.set_presentations(Vec::new(), None);
    }

    /// The `wallpaper_lookup` answer.
    pub(crate) fn wallpapers_arrived(
        mut self: Pin<&mut Self>,
        entries: &[pb::WallpaperEntry],
        displays: Option<&qobject::DisplayManager>,
    ) {
        for entry in entries {
            let map = rexon_core::we_wallpaper::entry_to_map(entry);
            self.as_mut().rust_mut().wallpapers.insert(entry.id.clone(), map);
        }
        self.rebuild_rows(displays);
    }

    fn set_presentations(
        mut self: Pin<&mut Self>,
        presentations: Vec<pb::WallpaperPresentationInfo>,
        displays: Option<&qobject::DisplayManager>,
    ) {
        let before = self.rust().presentations.len();
        let after = presentations.len();
        self.as_mut().rust_mut().presentations = presentations;
        self.as_mut().rebuild_rows(displays);
        if before != after {
            self.as_mut().count_changed();
        }
    }

    fn target_summary(
        presentation: &pb::WallpaperPresentationInfo,
        displays: Option<&qobject::DisplayManager>,
    ) -> String {
        use pb::wallpaper_presentation_target::Target;
        let labels: Vec<String> = presentation
            .targets
            .iter()
            .filter_map(|target| match target.target.as_ref()? {
                Target::DisplayId(id) => Some(
                    displays
                        .and_then(|displays| displays.display_label_of(*id))
                        .unwrap_or_else(|| {
                            presentation_fallback_label("Display #%1", &id.to_string())
                        }),
                ),
                Target::CanvasId(id) => Some(
                    displays
                        .and_then(|displays| displays.canvas_name_of(id))
                        .unwrap_or_else(|| presentation_fallback_label("Canvas %1", id)),
                ),
            })
            .collect();
        core_weq::presentation_target_summary(&labels)
    }

    /// Rebuild the rows: from the snapshot, the known wallpapers and the
    /// current names of displays and canvases. Called on a change of
    /// displays/canvases too — the target labels depend on them.
    pub(crate) fn rebuild_rows(self: Pin<&mut Self>, displays: Option<&qobject::DisplayManager>) {
        let rows: Vec<NowPlayingRow> = {
            let rust = self.rust();
            rust.presentations
                .iter()
                .map(|presentation| {
                    let id = &presentation.wallpaper_id;
                    // Until the daemon answers, the identifier serves as the name.
                    let wallpaper = rust.wallpapers.get(id).cloned().unwrap_or_else(|| {
                        let mut stub = serde_json::Map::new();
                        stub.insert("id_proto".into(), serde_json::Value::String(id.clone()));
                        stub.insert("name".into(), serde_json::Value::String(id.clone()));
                        stub
                    });
                    NowPlayingRow {
                        wallpaper_id: id.clone(),
                        wallpaper,
                        target_summary: Self::target_summary(presentation, displays),
                        state: presentation.state,
                    }
                })
                .collect()
        };
        let model = self.rust().model;
        if model.is_null() {
            return;
        }
        unsafe { Pin::new_unchecked(&mut *model.cast::<qobject::NowPlayingModel>()) }.replace(rows);
    }
}

// ============================================================================
// LibraryManager
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryInfo {
    pub id: i64,
    /// waywallen 0.5.0: the library name (`LibraryInstance.name`).
    pub name: String,
    pub path: String,
    pub plugin_name: String,
    /// waywallen 0.6.0: the library's setting values
    /// (`LibraryInstance.settings`; edited by `EditLibraryPage`).
    pub settings: BTreeMap<String, String>,
}

impl From<&pb::LibraryInstance> for LibraryInfo {
    fn from(l: &pb::LibraryInstance) -> Self {
        Self {
            id: l.id,
            name: l.name.clone(),
            path: l.path.clone(),
            plugin_name: l.plugin_name.clone(),
            settings: l.settings.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }
}

#[derive(Default)]
pub struct LibraryManagerRust {
    by_id: BTreeMap<i64, LibraryInfo>,
    /// Whether the first full list (`LibrarySnapshot`) has arrived. As in the
    /// original, it never goes back to `false`.
    initialized: bool,
}

impl LibraryManagerRust {
    /// Apply a daemon event. `true` means the list (or `initialized`) changed
    /// and QML must be notified. A full list always notifies: in the original
    /// `replaceAll` sends `librariesChanged` unconditionally.
    fn apply_event(&mut self, event: &pb::Event) -> bool {
        use pb::event::Payload;
        match event.payload.as_ref() {
            Some(Payload::LibrarySnapshot(snap)) => {
                self.by_id = snap
                    .libraries
                    .iter()
                    .map(|l| (l.id, LibraryInfo::from(l)))
                    .collect();
                self.initialized = true;
                true
            }
            Some(Payload::LibraryChanged(changed)) => {
                let Some(l) = changed.library.as_ref() else {
                    return false;
                };
                self.by_id.insert(l.id, LibraryInfo::from(l));
                true
            }
            Some(Payload::LibraryRemoved(removed)) => self.by_id.remove(&removed.id).is_some(),
            _ => false,
        }
    }
}

impl qobject::LibraryManager {
    fn libraries(&self) -> qobject::QList_QVariant {
        let mut out = QList::<QVariant>::default();
        for lib in self.rust().by_id.values() {
            let mut map = QMap::<QMapPair_QString_QVariant>::default();
            map.insert(QString::from("id"), QVariant::from(&lib.id));
            map.insert(
                QString::from("name"),
                QVariant::from(&QString::from(&lib.name)),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(&QString::from(&lib.path)),
            );
            map.insert(
                QString::from("pluginName"),
                QVariant::from(&QString::from(&lib.plugin_name)),
            );
            let mut settings = QMap::<QMapPair_QString_QVariant>::default();
            for (key, value) in &lib.settings {
                settings.insert(QString::from(key), QVariant::from(&QString::from(value)));
            }
            map.insert(QString::from("settings"), QVariant::from(&settings));
            out.append(QVariant::from(&map));
        }
        out
    }

    fn count(&self) -> i32 {
        self.rust().by_id.len() as i32
    }

    fn initialized(&self) -> bool {
        self.rust().initialized
    }

    fn notify(mut self: Pin<&mut Self>) {
        self.as_mut().libraries_changed();
        self.as_mut().count_changed();
    }

    pub(crate) fn handle_event(mut self: Pin<&mut Self>, event: &pb::Event) {
        if self.as_mut().rust_mut().apply_event(event) {
            self.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(renderer: &str, active: bool) -> LinkInfo {
        LinkInfo {
            renderer_id: renderer.to_string(),
            z_order: 0,
            active,
        }
    }

    fn condition(kind: &str) -> core_weq::RuntimeCondition {
        core_weq::RuntimeCondition {
            kind: kind.to_string(),
            ..Default::default()
        }
    }

    /// waywallen 0.5.0: the playlist status carries `waiting` (a smart playlist
    /// is waiting for suitable wallpapers) and `error`; without an active
    /// playlist there is no status at all — like
    /// `Display::playlistStatusFromPb`.
    #[test]
    fn playlist_status_carries_waiting_and_error() {
        let pb_status = pb::PlaylistDisplayStatus {
            active_id: 5,
            count: 0,
            waiting: true,
            error: "выборка пуста".into(),
            ..Default::default()
        };
        let status = PlaylistStatusInfo::from_pb(&pb_status).expect("плейлист активен");
        assert!(status.waiting);
        assert_eq!(status.error, "выборка пуста");
        assert_eq!(status.active_id, 5);
        let idle = pb::PlaylistDisplayStatus {
            active_id: 0,
            waiting: true,
            ..Default::default()
        };
        assert_eq!(PlaylistStatusInfo::from_pb(&idle), None);
    }

    /// A canvas collects links, conditions and the playlist only from ITS OWN
    /// displays: links are merged by renderer (shown on at least one — `active`),
    /// conditions do not repeat, the playlist is taken from the first one that
    /// has it.
    #[test]
    fn canvas_runtime_merges_its_displays_only() {
        let links_a = [link("r2", false), link("r1", false)];
        let links_b = [link("r1", true)];
        let links_other = [link("r9", true)];
        let cond_a = [condition("loading")];
        let cond_b = [condition("loading"), condition("hang")];
        let status = PlaylistStatusInfo {
            active_id: 7,
            ..Default::default()
        };
        let views = [
            CanvasDisplayView {
                canvas_id: "c1",
                links: &links_a,
                conditions: &cond_a,
                active_playlist_id: 0,
                playlist_status: None,
            },
            CanvasDisplayView {
                canvas_id: "other",
                links: &links_other,
                conditions: &[],
                active_playlist_id: 3,
                playlist_status: None,
            },
            CanvasDisplayView {
                canvas_id: "c1",
                links: &links_b,
                conditions: &cond_b,
                active_playlist_id: 7,
                playlist_status: Some(&status),
            },
        ];
        let runtime = canvas_runtime("c1", &views);
        assert_eq!(runtime.links, vec![link("r1", true), link("r2", false)]);
        assert_eq!(
            runtime.conditions,
            vec![condition("loading"), condition("hang")]
        );
        assert_eq!(runtime.active_playlist_id, 7);
        assert_eq!(runtime.playlist_status, Some(status.clone()));

        // A canvas without displays — empty, not someone else's.
        assert_eq!(canvas_runtime("none", &views), CanvasRuntime::default());
    }

    fn library(id: i64, name: &str, path: &str) -> pb::LibraryInstance {
        pb::LibraryInstance {
            id,
            path: path.to_string(),
            plugin_name: "we".to_string(),
            name: name.to_string(),
            settings: [("recursive".to_string(), "true".to_string())].into(),
        }
    }

    fn event(payload: pb::event::Payload) -> pb::Event {
        pb::Event {
            payload: Some(payload),
        }
    }

    /// waywallen 0.5.0, `ui/src/objmodel/library.cpp`: `initialized` is raised
    /// by the first full list and is never lowered again; the library name comes
    /// with the snapshot and with `LibraryChanged`; changes before the snapshot
    /// do not raise `initialized`.
    #[test]
    fn library_manager_initialized_and_name() {
        use pb::event::Payload;
        let mut lm = LibraryManagerRust::default();
        assert!(!lm.initialized, "до первого списка — не знаем");

        assert!(lm.apply_event(&event(Payload::LibraryChanged(pb::LibraryChanged {
            library: Some(library(5, "Ранняя", "/r")),
        }))));
        assert!(!lm.initialized, "одиночное изменение — не полный список");

        assert!(lm.apply_event(&event(Payload::LibrarySnapshot(pb::LibrarySnapshot {
            libraries: vec![library(3, "Мастерская", "/w"), library(1, "", "/p")],
        }))));
        assert!(lm.initialized);
        let rows: Vec<&LibraryInfo> = lm.by_id.values().collect();
        assert_eq!(
            rows,
            vec![
                &LibraryInfo {
                    id: 1,
                    name: String::new(),
                    path: "/p".into(),
                    plugin_name: "we".into(),
                    settings: [("recursive".to_string(), "true".to_string())].into(),
                },
                &LibraryInfo {
                    id: 3,
                    name: "Мастерская".into(),
                    path: "/w".into(),
                    plugin_name: "we".into(),
                    settings: [("recursive".to_string(), "true".to_string())].into(),
                },
            ],
            "снимок заменяет список целиком, порядок по номеру"
        );

        // A rename by the daemon arrives as LibraryChanged.
        assert!(lm.apply_event(&event(Payload::LibraryChanged(pb::LibraryChanged {
            library: Some(library(3, "Steam", "/w")),
        }))));
        assert_eq!(lm.by_id[&3].name, "Steam");

        // An empty snapshot: there are no libraries, but the list is already
        // known.
        assert!(lm.apply_event(&event(Payload::LibrarySnapshot(pb::LibrarySnapshot {
            libraries: vec![],
        }))));
        assert!(lm.by_id.is_empty());
        assert!(lm.initialized, "назад в false не возвращается");

        assert!(!lm.apply_event(&event(Payload::LibraryRemoved(pb::LibraryRemoved { id: 9 }))),
            "удаление неизвестной — без оповещения");
        assert!(!lm.apply_event(&event(Payload::LibraryChanged(pb::LibraryChanged {
            library: None,
        }))));
    }
}
