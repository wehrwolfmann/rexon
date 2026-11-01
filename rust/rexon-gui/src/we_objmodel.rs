// Живые модели App: рендереры, дисплеи, библиотеки и видеокарты.
//
// Порт
// `ui/src/objmodel/{renderer,display,library}.cpp` waywallen (MIT,
// (c) hypengw; атрибуция в NOTICE).
//
// Все пять классов в ОДНОМ мосте намеренно: cxx печатает шим-обёртку на
// каждое объявление `extern "C++"`, собранную по имени C++-функции, и
// фабрика дочерних объектов (`rexon_new_renderer`/`rexon_new_display`),
// объявленная в двух мостах крейта, дала бы `duplicate symbol` на линковке.
// Одного `#[qobject]` на файл cxx-qt не требует — теряется только шорткат
// `Self`, которым здесь всё равно не пользуемся.
//
// Кто кормит модели: события демона (`*_snapshot`/`*_changed`/`*_removed`)
// приходят сюда сырым кадром через App, плюс ответ `renderer_list` от
// `status_queries::RendererListQuery` — тем же сырым кадром через шов
// `renderers_replace_frame`.
//
// Что приехало со схемой v0.3.5 и живёт здесь: состояние рендерера
// (`RendererState` вместо строки `status`), его условия и метки
// (`runtimeConditions`/`runtimeTags`), а у дисплея — принадлежность холсту
// (`canvasId`/`canvasRect`/`canvasOverlapCount`), ключ настроек и свои
// условия. Строку `status` и `running` собирает ядро
// (`we_query::renderer_status`) — один движок на оба пути прихода.
//
// Холсты (`Canvas`) — тоже здесь: демон шлёт `canvas_snapshot` и при
// подключении, и на каждое изменение, поэтому отдельного запроса списка нет.
// Данные холста (члены, охват, раскладка) приходят снимком; связи, плейлист
// и условия холста собираются из его дисплеев и пересчитываются после
// каждой смены дисплеев или плейлистов (`refresh_canvas_runtime`).
//
// Видеокарты (`Gpu`/`GpuManager`) — единственная модель без событий: демон
// перечисляет карты один раз при старте, и список приходит ответом
// `gpu_list`, который App запрашивает на каждом подключении (`we_app.rs`).
//
// Текущие показы обоев (`PresentationManager` и его модель строк
// `NowPlayingModel`) — карточка «Текущие обои» в рейке вкладки. Снимок
// показов демон шлёт при подключении и на каждую смену; обрыв связи список
// чистит сразу. К строке прикладываются обоина (её App дозапрашивает
// `wallpaper_lookup`) и подпись целей — имена дисплеев и холстов.

/// Мост cxx-qt: Renderer, RendererManager, Display, Canvas, DisplayManager,
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
        /// База модели строк карточки. Без `#[qobject]` — как в остальных
        /// мостах крейта с моделями (`remote_list_model.rs`): иначе шимы
        /// приведения типа напечатаются второй раз и не слинкуются.
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

        /// Создать дочерний `Renderer`/`Display`/`Canvas`/`Gpu` — `new` живёт
        /// только в C++.
        ///
        /// # Safety
        /// `parent` — живой QObject-менеджер (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_renderer(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_display(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_canvas(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_gpu(parent: *mut QObject) -> *mut QObject;
        /// Модель строк карточки «Текущие обои» — ребёнок своего менеджера.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_now_playing_model(parent: *mut QObject) -> *mut QObject;

        /// Строка вендорного каталога перевода по её контексту — подписи
        /// «Дисплей №…» и «Холст …» для целей, которых нет в списках.
        fn rexon_we_vendor_tr(context: &QString, source: &QString) -> QString;

        /// Снести дочерний объект отложенно (`deleteLater`), как в
        /// оригинале: удалять прямо сейчас нельзя — на объект могут быть
        /// живые привязки QML в текущем кадре.
        ///
        /// # Safety
        /// `object` — живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_delete_child_later(object: *mut QObject);

        /// Указатель на QObject значением QVariant: так вендорный QML
        /// получает списки `renderers`/`displays`/`gpus`.
        ///
        /// # Safety
        /// `object` — живой QObject либо nullptr.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_object_to_variant(object: *mut QObject) -> QVariant;
        // (объявление живёт в rexon_we_objmodel.h — рядом с фабрикой)
    }

    // ── Renderer ──────────────────────────────────────────────────────────
    extern "RustQt" {
        /// Один рендерер как QObject (зеркало `proto::RendererInstance`).
        /// Создаётся и умирает только вместе с менеджером — из QML
        /// экземпляр не собрать, как и у оригинала (`QML_UNCREATABLE`).
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(QString, id, READ = id, CONSTANT)]
        #[qproperty(u32, fps, READ = fps, NOTIFY = fps_changed)]
        #[qproperty(QString, status, READ = status, NOTIFY = status_changed)]
        /// Живой процесс: играет, на паузе или без звука. Порт
        /// `Renderer::running` вендора — `StatusPage.qml:347` красит им
        /// значок, а `:331` прячет придержанные процессы.
        #[qproperty(bool, running, READ = running, NOTIFY = status_changed)]
        /// Процесс придержан после остановки (`Renderer::keep`).
        #[qproperty(bool, keep, READ = keep, NOTIFY = status_changed)]
        /// Причина последнего выхода (`Renderer::lastExitReason`).
        #[qproperty(QString, last_exit_reason, cxx_name = "lastExitReason",
                    READ = last_exit_reason, NOTIFY = status_changed)]
        /// Поколение процесса (`Renderer::processGeneration`).
        /// ⚠️ `f64`, а не `u64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`). Поле внутри остаётся `u64`.
        #[qproperty(f64, process_generation, cxx_name = "processGeneration",
                    READ = process_generation, NOTIFY = status_changed)]
        /// Что с рендерером прямо сейчас: загрузка, ожидание, зависание
        /// (`StatusPage.qml:281`). Порт `Renderer::runtimeConditions`.
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = runtime_conditions, NOTIFY = runtime_conditions_changed)]
        /// Пары ключ-значение от рендерера (`StatusPage.qml`, плашки).
        #[qproperty(QList_QVariant, runtime_tags, cxx_name = "runtimeTags",
                    READ = runtime_tags, NOTIFY = runtime_tags_changed)]
        #[qproperty(QString, name, READ = name, NOTIFY = name_changed)]
        #[qproperty(u32, pid, READ = pid, NOTIFY = pid_changed)]
        #[qproperty(u32, texture_width, cxx_name = "textureWidth",
                    READ = texture_width, NOTIFY = texture_size_changed)]
        #[qproperty(u32, texture_height, cxx_name = "textureHeight",
                    READ = texture_height, NOTIFY = texture_size_changed)]
        /// Карта рисовальщика (номер узла DRM). ⚠️ НЕ постоянное свойство:
        /// рисовальщик рождается с нулями и называет свою карту позже,
        /// обновлением, — значок на открытой странице «Состояние» ждёт
        /// именно этого сигнала (`drmRenderChanged` оригинала).
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

        /// Рендерер по идентификатору или nullptr.
        #[qinvokable]
        fn get(self: &RendererManager, id: &QString) -> *mut QObject;
    }

    // ── Display ───────────────────────────────────────────────────────────
    extern "RustQt" {
        /// Один дисплей как QObject (зеркало `proto::DisplayInfo`).
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        /// ⚠️ `f64`, а не `u64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`). Номера дисплеев у демона мелкие,
        /// потери точности нет.
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
        /// waywallen 0.4.2: дисплей поставлен на паузу вручную (кнопкой
        /// страницы «Дисплеи», `DisplayPauseSetQuery`).
        #[qproperty(bool, manual_paused, cxx_name = "manualPaused",
                    READ = manual_paused, NOTIFY = pause_changed)]
        /// waywallen 0.4.2: дисплей стоит на паузе по любой причине —
        /// вручную или по правилам автопаузы.
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
        /// ⚠️ `f64`, а не `i64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`).
        #[qproperty(f64, active_playlist_id, cxx_name = "activePlaylistId",
                    READ = active_playlist_id, NOTIFY = playlist_status_changed)]
        #[qproperty(QMap_QString_QVariant, playlist_status, cxx_name = "playlistStatus",
                    READ = playlist_status, NOTIFY = playlist_status_changed)]
        /// Годится ли дисплей в отдельную цель применения. У демона это
        /// `selectable_target = canvas_id.is_none()` (router/mod.rs:2666):
        /// дисплей, отданный холсту, целью не предлагают — цель тогда
        /// холст. Формулу повторяем, читая `canvas_id` (см. `DisplayInfo`).
        #[qproperty(bool, selectable_target, cxx_name = "selectableTarget",
                    READ = selectable_target, NOTIFY = canvas_changed)]
        /// Холст, которому отдан дисплей («» — дисплей сам по себе).
        #[qproperty(QString, canvas_id, cxx_name = "canvasId",
                    READ = canvas_id, NOTIFY = canvas_changed)]
        /// Место дисплея внутри холста (`x`/`y`/`width`/`height`).
        #[qproperty(QMap_QString_QVariant, canvas_rect, cxx_name = "canvasRect",
                    READ = canvas_rect, NOTIFY = canvas_changed)]
        /// Сколько соседей по холсту перекрывают этот дисплей.
        #[qproperty(u32, canvas_overlap_count, cxx_name = "canvasOverlapCount",
                    READ = canvas_overlap_count, NOTIFY = canvas_changed)]
        /// Ключ дисплея в настройках демона — им подписан кусок
        /// `config.toml` и им же холст перечисляет своих членов.
        #[qproperty(QString, settings_key, cxx_name = "settingsKey",
                    READ = settings_key, NOTIFY = settings_key_changed)]
        /// Что с дисплеем прямо сейчас (`DisplaysPage.qml:302`).
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = runtime_conditions, NOTIFY = runtime_conditions_changed)]
        /// waywallen 0.4.4: умеет ли клиент дисплея исключения окон
        /// (`WindowExclusionSupport`).
        #[qproperty(i32, window_exclusion_support, cxx_name = "windowExclusionSupport",
                    READ = window_exclusion_support,
                    NOTIFY = window_exclusion_support_changed)]
        /// waywallen 0.4.4: сколько правил исключения дисплей не применит —
        /// по нему страница «Исключённые окна» предупреждает человека.
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
        /// Смена принадлежности холсту: у оригинала на этом сигнале висят
        /// `canvasId`/`canvasRect`/`canvasOverlapCount`/`selectableTarget`.
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
        /// Один холст как QObject (зеркало `proto::CanvasInfo`): несколько
        /// дисплеев, на которые обоя ложится одним полотном. Из QML не
        /// создаётся — холстами владеет `DisplayManager`.
        #[qobject]
        #[qml_element]
        #[qml_uncreatable]
        #[qproperty(QString, id, cxx_name = "id", READ = canvas_object_id, CONSTANT)]
        #[qproperty(QString, name, READ = canvas_name, NOTIFY = changed)]
        /// То же имя: у холста, в отличие от дисплея, псевдонима нет.
        #[qproperty(QString, display_label, cxx_name = "displayLabel",
                    READ = canvas_name, NOTIFY = changed)]
        /// Члены холста: `settingsKey`, `rect`, `minimumScaleTo`,
        /// `aspectLocked`, `displayIds`, `onlineCount`, `overlap`.
        #[qproperty(QList_QVariant, members, READ = members, NOTIFY = changed)]
        /// Охват холста (`x`/`y`/`width`/`height`); демон его не прислал —
        /// карта пустая.
        #[qproperty(QMap_QString_QVariant, extent, READ = extent, NOTIFY = changed)]
        #[qproperty(u32, width, READ = extent_width, NOTIFY = changed)]
        #[qproperty(u32, height, READ = extent_height, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, layout_override, cxx_name = "layoutOverride",
                    READ = canvas_layout_override, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, effective_layout, cxx_name = "effectiveLayout",
                    READ = canvas_effective_layout, NOTIFY = changed)]
        /// Связи с рендерерами, слитые по всем дисплеям холста.
        #[qproperty(QList_QVariant, links, READ = canvas_links, NOTIFY = runtime_changed)]
        /// ⚠️ `f64`, а не `i64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`).
        #[qproperty(f64, active_playlist_id, cxx_name = "activePlaylistId",
                    READ = canvas_active_playlist_id, NOTIFY = runtime_changed)]
        #[qproperty(QMap_QString_QVariant, playlist_status, cxx_name = "playlistStatus",
                    READ = canvas_playlist_status, NOTIFY = runtime_changed)]
        #[qproperty(QList_QVariant, runtime_conditions, cxx_name = "runtimeConditions",
                    READ = canvas_runtime_conditions, NOTIFY = runtime_changed)]
        #[qproperty(QString, wallpaper_id, cxx_name = "wallpaperId",
                    READ = wallpaper_id, NOTIFY = changed)]
        /// Номер правки холста: с ним уходят изменение и удаление, демон
        /// отказывает, если холст успели поменять. ⚠️ `f64`, а не `u64` —
        /// по той же причине, что и выше.
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
        /// Сменилось то, что прислал демон о самом холсте.
        #[qsignal]
        fn changed(self: Pin<&mut Canvas>);
        /// Сменилось собранное из дисплеев холста: связи, плейлист, условия.
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
        /// Холсты по имени, затем по идентификатору (объекты `Canvas`).
        #[qproperty(QList_QVariant, canvases, READ = canvases,
                    NOTIFY = canvases_changed)]
        /// Номер правки всего набора холстов из последнего снимка демона.
        /// ⚠️ `f64`, а не `u64` (разбор — `src/qvalue.rs`).
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

        /// Дисплей по идентификатору или nullptr.
        ///
        /// ⚠️ `id` принимаем `f64`: 64-битное целое в подписи, видимой QML,
        /// не выпускаем (разбор — `src/qvalue.rs`). Вендорный QML зовёт
        /// `W.App.displayManager.get(displayId)` обычным числом JS.
        #[qinvokable]
        fn get(self: &DisplayManager, id: f64) -> *mut QObject;

        /// Холст по идентификатору или nullptr на незнакомый id.
        #[qinvokable]
        #[cxx_name = "getCanvas"]
        fn get_canvas(self: &DisplayManager, id: &QString) -> *mut QObject;
    }

    // ── Gpu ───────────────────────────────────────────────────────────────
    extern "RustQt" {
        /// Одна видеокарта (зеркало `proto::GpuInfo`). Демон перечисляет
        /// карты один раз при старте и больше о них не сообщает, поэтому
        /// все свойства постоянны — как у оригинала.
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
        /// Видеокарты машины в порядке демона (объекты `Gpu`): значок карты
        /// у рисовальщика и у дисплея, выбор карты в настройках.
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

        /// Карта по номеру узла DRM (узла отрисовки или основного) либо
        /// nullptr: узел неизвестен (`major == 0`) или такой карты нет.
        #[qinvokable]
        fn find(self: &GpuManager, major: u32, minor: u32) -> *mut QObject;
    }

    // ── NowPlayingModel ───────────────────────────────────────────────────
    extern "RustQt" {
        /// Строки карточки «Текущие обои»: роли `wallpaper`, `wallpaperId`,
        /// `targetSummary`, `state` — их читает
        /// `component/SidebarNowPlaying.qml`. Живёт только у своего
        /// менеджера.
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

        /// Строка целиком — так карточка узнаёт, какую обоину открыть.
        #[qinvokable]
        fn item(self: &NowPlayingModel, row: i32) -> QMap_QString_QVariant;

        /// # Safety
        /// Требует парного `end_reset_model`.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut NowPlayingModel>);
        /// # Safety
        /// Парный к `begin_reset_model`.
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
        /// Текущие показы обоев: `count` и `model` — ровно то, что читает
        /// рейка (`W.App.presentationManager`).
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
        /// Список библиотек обоей (SourceManagePage).
        ///
        /// ⚠️ У оригинала строка списка — QObject `Library` с четырьмя
        /// свойствами; здесь она остаётся картой. Наблюдаемого различия
        /// нет: вендорный QML читает у строки только `id`/`name`/`path`/
        /// `pluginName` точкой, а карта отвечает на это ровно так же.
        /// Заводить QObject на строку без единого сигнала — лишний узел
        /// жизненного цикла ради одинакового результата. Смена имени
        /// (`LibraryChanged`) всё равно приходит `librariesChanged` — список
        /// пересобирается целиком.
        ///
        /// `initialized` (waywallen 0.5.0) — пришёл ли уже первый полный
        /// список: до него «библиотек нет» значит «ещё не знаем», и
        /// SourceManagePage не показывает пустое состояние.
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
        /// Отдельный сигнал нашего моста, оставленный ради привязок,
        /// которые слушали именно его.
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

/// Поля `RendererInstance`, приходящие двумя путями: событием демона
/// (протобуф) и ответом `renderer_list` (карта от Python-запроса). Один
/// нормализатор на оба — правило #12.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererInfo {
    pub id: String,
    pub fps: u32,
    /// Строка состояния. С v0.3.5 демон её не шлёт — она собирается из
    /// дерева `RendererState` общим движком ядра (`we_query::renderer_status`),
    /// одним на все пути прихода рендерера.
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

/// `RuntimeCondition` → карта для QML (ключи вендора).
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

/// `RendererRuntimeTag` → карта для QML.
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

    /// Первичное заполнение: сигналов не шлём, объект ещё никому не отдан.
    fn install(mut self: Pin<&mut Self>, info: RendererInfo) {
        self.as_mut().rust_mut().info = info;
    }

    /// Порт `Renderer::updateFrom`: сигнал шлётся ТОЛЬКО по изменившемуся
    /// свойству, а не общим «что-то поменялось».
    fn update_from(mut self: Pin<&mut Self>, info: RendererInfo) {
        let old = self.rust().info.clone();
        self.as_mut().rust_mut().info = info;
        let new = self.rust().info.clone();
        if old.fps != new.fps {
            self.as_mut().fps_changed();
        }
        // Один сигнал на всё состояние — как `stateChanged` вендора: строка
        // `status` может не измениться (два раза «stopped»), а придержанность
        // процесса или причина выхода — измениться.
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

/// Указатели сырые: владеет объектами Qt (они дети менеджера), а порядок и
/// поиск держим здесь — как `m_ordered`/`m_by_id` оригинала.
#[derive(Default)]
pub struct RendererManagerRust {
    by_id: BTreeMap<String, *mut qobject::QObject>,
}

/// # Safety
/// `ptr` — живой `Renderer`, созданный фабрикой этого моста.
unsafe fn renderer_pin<'a>(ptr: *mut qobject::QObject) -> Pin<&'a mut qobject::Renderer> {
    unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Renderer>()) }
}

impl qobject::RendererManager {
    fn renderers(&self) -> qobject::QList_QVariant {
        // BTreeMap уже упорядочен по идентификатору — тот самый
        // `std::sort` по `id`, который делает оригинал.
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

    /// Порт `RendererManager::replaceAll`: чего нет в снимке — снести,
    /// остальное обновить, ровно один `renderersChanged` на пачку.
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

    /// Порт `RendererManager::upsert`.
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

    /// Порт `RendererManager::remove`.
    fn remove(mut self: Pin<&mut Self>, id: &str) {
        let Some(ptr) = self.as_mut().rust_mut().by_id.remove(id) else {
            return;
        };
        unsafe { qobject::rexon_delete_child_later(ptr) };
        self.as_mut().renderers_changed();
    }

    /// Разбор события демона (`handleEvent` оригинала).
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

    /// Ответ `renderer_list` — полная замена списка.
    ///
    /// На вход тот же СЫРОЙ кадр демона, что разбирает сам запрос: путь
    /// один и тот же протобуф, что и у событий, поэтому нормализатор тоже
    /// один — `RendererInfo::from_pb` (правило #12). Прежний шов возил
    /// сюда JSON-карты питоновского моста и требовал второго,
    /// «обратного» разборщика; он удалён вместе с мостом.
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
    /// waywallen 0.4.4: отражение (`Flip`).
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
    /// waywallen 0.4.4: отражение задано у дисплея и само отражение.
    pub flip_set: bool,
    pub flip: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkInfo {
    pub renderer_id: String,
    pub z_order: i32,
    /// Связь сейчас показывается (поле 3 `DisplayLinkInfo`). Холст по нему
    /// сливает связи своих дисплеев.
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
    /// waywallen 0.5.0: умный плейлист ждёт подходящих обоев (выборка пока
    /// пуста) — `PlaylistDisplayStatus.waiting`.
    pub waiting: bool,
    /// waywallen 0.5.0: текст ошибки плейлиста на этом дисплее; пусто —
    /// ошибки нет.
    pub error: String,
}

impl PlaylistStatusInfo {
    /// Порт `Display::playlistStatusFromPb`: `activeId <= 0` — статуса нет.
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
    /// Холст, которому отдан дисплей («» — дисплей сам по себе).
    /// Поле 17 `DisplayInfo` схемы v0.3.5.
    pub canvas_id: String,
    /// Место дисплея внутри холста (поле 18).
    pub canvas_rect: CanvasRect,
    /// Сколько соседей по холсту его перекрывают (поле 19).
    pub canvas_overlap_count: u32,
    /// Ключ дисплея в настройках демона (поле 16).
    pub settings_key: String,
    /// Что с дисплеем прямо сейчас (поле 14).
    pub conditions: Vec<core_weq::RuntimeCondition>,
    /// Ручная пауза дисплея (поле 21, waywallen 0.4.2).
    pub manual_paused: bool,
    /// Пауза по любой причине (поле 22, waywallen 0.4.2).
    pub effective_paused: bool,
    /// Умеет ли клиент дисплея исключения окон (поле 23, waywallen 0.4.4):
    /// `WindowExclusionSupport` — 0 по-старому, 1 умеет, 2 частично.
    pub window_exclusion_support: i32,
    /// Сколько правил исключения этот дисплей применить не может
    /// (поле 24, waywallen 0.4.4).
    pub unsupported_window_exclusions: u32,
}

/// Прямоугольник холста. Своя копия `pb::CanvasRect`, потому что
/// prost-структуры не выводят `Eq`, а весь диффер моделей стоит на нём.
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
    // waywallen 0.4.4: `flip` в действующей раскладке и раскладке дисплея.
    map.insert(QString::from("flip"), QVariant::from(&layout.flip));
    map
}

/// Связи с рендерерами списком карт (`rendererId`/`zOrder`/`active`) — один
/// вид и для дисплея, и для холста.
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

/// Статус плейлиста картой; плейлиста нет — карта пустая. Один вид и для
/// дисплея, и для холста.
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
    // waywallen 0.5.0 (`display.cpp`): ключи есть всегда, пока есть статус —
    // `PlaylistListSheetState.qml` читает `status.error` и `status.waiting`.
    map.insert(QString::from("waiting"), QVariant::from(&status.waiting));
    map.insert(
        QString::from("error"),
        QVariant::from(&QString::from(&status.error)),
    );
    map
}

/// Прямоугольник холста картой (`x`/`y`/`width`/`height`).
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
        // waywallen 0.4.4: отражение в оверрайде дисплея.
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

    /// Формула демона один в один: цель выбираема, пока дисплей не отдан
    /// холсту (`selectable_target = canvas_id.is_none()`,
    /// `src/wallframe/routing/router/mod.rs:2666` вендора на v0.3.5).
    fn selectable_target(&self) -> bool {
        self.rust().info.canvas_id.is_empty()
    }

    fn canvas_id(&self) -> QString {
        QString::from(&self.rust().info.canvas_id)
    }

    /// Прямоугольник холста картой — форма как у `scenePosition` вендора
    /// (`DisplaysPage.qml` читает `x`/`y`/`width`/`height`). Холста нет —
    /// карта нулевая, а не отсутствующая: QML читает поля безусловно.
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

    /// Порт `Display::updateFrom` — по сигналу на каждое изменившееся поле.
    fn update_from(mut self: Pin<&mut Self>, info: DisplayInfo) {
        let old = self.rust().info.clone();
        self.as_mut().rust_mut().info = info;
        let new = self.rust().info.clone();
        // waywallen 0.4.4: один `windowExclusionSupportChanged` на оба поля,
        // и первым — как у `Display::updateFrom` оригинала.
        if old.window_exclusion_support != new.window_exclusion_support
            || old.unsupported_window_exclusions != new.unsupported_window_exclusions
        {
            self.as_mut().window_exclusion_support_changed();
        }
        // Оригинал (v0.4.2) шлёт один `pauseChanged` на оба флага.
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

    /// Порт `Display::updatePlaylistStatus`: `activeId <= 0` — статус пуст.
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

/// Один член холста — зеркало `proto::CanvasMemberInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasMemberInfo {
    /// Ключ дисплея в настройках демона: член холста — это место под
    /// дисплей, а не сам дисплей, и живёт он и без подключённого экрана.
    pub settings_key: String,
    pub rect: Option<CanvasRect>,
    /// Наибольший живой размер поверхности члена (ширина, высота); члена
    /// нет в сети — демон поле не шлёт.
    pub minimum_scale_to: Option<(u32, u32)>,
    pub aspect_locked: bool,
    /// Подключённые сейчас дисплеи этого члена.
    pub display_ids: Vec<u64>,
}

/// То, что демон шлёт о холсте (`proto::CanvasInfo`).
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

    /// Сколько дисплеев холста сейчас подключено (по всем членам).
    fn online_count(&self) -> usize {
        self.members.iter().map(|m| m.display_ids.len()).sum()
    }
}

/// Собранное из дисплеев холста: демон этого о холсте не шлёт.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanvasRuntime {
    pub links: Vec<LinkInfo>,
    pub active_playlist_id: i64,
    pub playlist_status: Option<PlaylistStatusInfo>,
    pub conditions: Vec<core_weq::RuntimeCondition>,
}

/// То, что холсту нужно знать о дисплее.
pub struct CanvasDisplayView<'a> {
    pub canvas_id: &'a str,
    pub links: &'a [LinkInfo],
    pub conditions: &'a [core_weq::RuntimeCondition],
    pub active_playlist_id: i64,
    pub playlist_status: Option<&'a PlaylistStatusInfo>,
}

/// Порт `Canvas::updateRuntime`. Дисплеи идут по возрастанию id.
///
/// * связи сливаются по рендереру: остаётся первая встреченная, а `active`
///   поднимается, если связь показывается хоть на одном дисплее холста;
///   порядок итога — по идентификатору рендерера;
/// * условия — без повторов, в порядке появления;
/// * плейлист — первого дисплея холста, у которого он есть.
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
            // Прямоугольника или размера нет — карта пустая, а не
            // отсутствующая: разметка читает поля строки безусловно.
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
            // ⚠️ Номера дисплеев — `f64`, как `Display::id`: 64-битное целое
            // в QML не выпускаем, а разметка сравнивает их с `display.id`.
            let mut ids = QList::<QVariant>::default();
            for id in &member.display_ids {
                ids.append(QVariant::from(&crate::qvalue::u64_to_qml(*id)));
            }
            row.insert(QString::from("displayIds"), QVariant::from(&ids));
            let online = member.display_ids.len();
            row.insert(QString::from("onlineCount"), QVariant::from(&(online as i32)));
            // Два живых дисплея на одном месте холста.
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

    /// Оверрайд раскладки холста. Выравнивания (`align`) у холста нет —
    /// ключи только те, что задаёт `CanvasLayoutSetQuery`.
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

    /// Первичное заполнение: сигналов не шлём, объект ещё никому не отдан.
    fn install(mut self: Pin<&mut Self>, info: CanvasInfo) {
        self.as_mut().rust_mut().info = info;
    }

    /// Порт `Canvas::updateFrom`: один `changed` на любое отличие снимка.
    fn update_from(mut self: Pin<&mut Self>, info: CanvasInfo) {
        if self.rust().info == info {
            return;
        }
        self.as_mut().rust_mut().info = info;
        self.as_mut().changed();
    }

    /// Принять пересчитанное по дисплеям; сигнал — только при отличии.
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
    /// Холсты в порядке показа: по имени, затем по идентификатору.
    canvas_order: Vec<*mut qobject::QObject>,
    canvas_revision: u64,
}

/// # Safety
/// `ptr` — живой `Display`, созданный фабрикой этого моста.
unsafe fn display_pin<'a>(ptr: *mut qobject::QObject) -> Pin<&'a mut qobject::Display> {
    unsafe { Pin::new_unchecked(&mut *ptr.cast::<qobject::Display>()) }
}

/// # Safety
/// `ptr` — живой `Canvas`, созданный фабрикой этого моста.
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

    /// Подпись дисплея по идентификатору — та же, что в `displayLabel`.
    pub(crate) fn display_label_of(&self, id: u64) -> Option<String> {
        self.rust().by_id.get(&id).map(|ptr| {
            let display: &qobject::Display = unsafe { &*ptr.cast::<qobject::Display>() };
            let info = &display.rust().info;
            rexon_core::we_util::display_label(&info.alias, &info.name, info.id)
        })
    }

    /// Имя холста по идентификатору.
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

    /// Порт `DisplayManager::replaceCanvases`: снимок — полная замена.
    /// Знакомые холсты обновляются на месте (объект тот же — на нём висят
    /// привязки страницы), пропавшие сносятся, `canvasesChanged` шлётся на
    /// каждый снимок.
    fn replace_canvases(mut self: Pin<&mut Self>, infos: Vec<CanvasInfo>, revision: u64) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::DisplayManager = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::DisplayManager).cast()
        };
        let mut old = std::mem::take(&mut self.as_mut().rust_mut().canvas_by_id);
        let mut next: BTreeMap<String, *mut qobject::QObject> = BTreeMap::new();
        // Ключ порядка: (имя, идентификатор).
        let mut order: Vec<((String, String), *mut qobject::QObject)> = Vec::new();
        for info in infos {
            let key = (info.name.clone(), info.id.clone());
            // Повтор идентификатора в одном снимке: объект уже в `next`.
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

    /// Порт `DisplayManager::refreshCanvasRuntime`: связи, плейлист и
    /// условия каждого холста собрать заново из его дисплеев. Звать после
    /// ЛЮБОЙ смены дисплеев, плейлистов или самих холстов.
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
        // Ушёл дисплей с плейлистом — признак «есть активные» мог смениться.
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

    /// Порт `DisplayManager::replacePlaylistStatuses`: дисплеи вне списка
    /// получают пустой статус (иначе на экране висел бы мёртвый плейлист).
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

    /// Заполнить до того, как объект кому-либо отдан: свойства постоянны,
    /// сигналов у них нет.
    fn install(mut self: Pin<&mut Self>, info: pb::GpuInfo) {
        self.as_mut().rust_mut().info = info;
    }
}

/// Порядок — как прислал демон. `infos` и `objects` идут строка в строку:
/// первые — данные для поиска и сравнения, вторые — объекты, которыми
/// владеет Qt (они дети менеджера).
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

    /// Полная замена списка ответом `gpu_list`.
    ///
    /// ⚠️ Тот же список второй раз (переподключение к тому же демону)
    /// объекты НЕ пересоздаёт. Значок карты находит свой объект один раз,
    /// вызовом `find`, и за `gpusChanged` не следит: снеси мы объекты —
    /// значки у рисовальщиков и дисплеев пропали бы до пересоздания
    /// страницы, хотя карты те же.
    ///
    /// Возвращает, сменился ли список: App по этому признаку велит значкам
    /// найти свои карты заново (`gpuManagerChanged`).
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

/// Строка карточки «Текущие обои».
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlayingRow {
    pub wallpaper_id: String,
    /// Обоина в терминах QML (`we_wallpaper::entry_to_map`).
    pub wallpaper: serde_json::Map<String, serde_json::Value>,
    pub target_summary: String,
    pub state: i32,
}

/// Имена ролей — посимвольно те, что читает `SidebarNowPlaying.qml`.
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
        // У плоского списка дети есть только у корня.
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

    /// Заменить строки. Те же обои в том же порядке (приехали данные обоины
    /// или сменилась подпись цели) обновляются на месте — карточка не
    /// перестраивается и не теряет положение; иной состав — сброс модели.
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

/// Снимок показов демона и обоины, которые к нему уже известны.
pub struct PresentationManagerRust {
    presentations: Vec<pb::WallpaperPresentationInfo>,
    /// Обоины по идентификатору. Переживают обрыв связи: после
    /// переподключения карточка сразу показывает имя и превью, а свежий
    /// ответ демона их обновляет.
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

/// Строка вендорного перевода из контекста менеджера показов с подстановкой.
fn presentation_fallback_label(source: &str, id: &str) -> String {
    qobject::rexon_we_vendor_tr(
        &QString::from("waywallen::PresentationManager"),
        &QString::from(source),
    )
    .to_string()
    .replace("%1", id)
}

impl qobject::PresentationManager {
    fn count(&self) -> i32 {
        self.rust().presentations.len() as i32
    }

    fn model(&self) -> *mut qobject::QObject {
        self.rust().model
    }

    /// Завести модель строк. Зовёт App сразу после создания менеджера —
    /// раньше, чем свойство `model` прочтёт разметка.
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

    /// Новый снимок показов. Возвращает идентификаторы обоев, которые надо
    /// спросить у демона (все из снимка: известные могли измениться).
    pub(crate) fn replace_snapshot(
        mut self: Pin<&mut Self>,
        presentations: Vec<pb::WallpaperPresentationInfo>,
        displays: Option<&qobject::DisplayManager>,
    ) -> Vec<String> {
        let ids = core_weq::presentation_wallpaper_ids(&presentations);
        self.as_mut().set_presentations(presentations, displays);
        ids
    }

    /// Связь с демоном оборвана — показов нет, пока не придёт новый снимок.
    pub(crate) fn clear(self: Pin<&mut Self>) {
        self.set_presentations(Vec::new(), None);
    }

    /// Ответ `wallpaper_lookup`.
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

    /// Пересобрать строки: по снимку, известным обоинам и нынешним именам
    /// дисплеев и холстов. Зовётся и на смену дисплеев/холстов — подписи
    /// целей зависят от них.
    pub(crate) fn rebuild_rows(self: Pin<&mut Self>, displays: Option<&qobject::DisplayManager>) {
        let rows: Vec<NowPlayingRow> = {
            let rust = self.rust();
            rust.presentations
                .iter()
                .map(|presentation| {
                    let id = &presentation.wallpaper_id;
                    // Пока демон не ответил — именем служит идентификатор.
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
    /// waywallen 0.5.0: имя библиотеки (`LibraryInstance.name`).
    pub name: String,
    pub path: String,
    pub plugin_name: String,
}

impl From<&pb::LibraryInstance> for LibraryInfo {
    fn from(l: &pb::LibraryInstance) -> Self {
        Self {
            id: l.id,
            name: l.name.clone(),
            path: l.path.clone(),
            plugin_name: l.plugin_name.clone(),
        }
    }
}

#[derive(Default)]
pub struct LibraryManagerRust {
    by_id: BTreeMap<i64, LibraryInfo>,
    /// Пришёл ли первый полный список (`LibrarySnapshot`). Как у оригинала,
    /// назад в `false` не возвращается.
    initialized: bool,
}

impl LibraryManagerRust {
    /// Применить событие демона. `true` — список (или `initialized`)
    /// изменился и надо оповестить QML. Полный список всегда оповещает:
    /// у оригинала `replaceAll` шлёт `librariesChanged` безусловно.
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

    /// waywallen 0.5.0: статус плейлиста несёт `waiting` (умный плейлист
    /// ждёт подходящих обоев) и `error`; без активного плейлиста статуса нет
    /// вовсе — как `Display::playlistStatusFromPb`.
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

    /// Холст собирает связи, условия и плейлист только со СВОИХ дисплеев:
    /// связи сливаются по рендереру (показ хоть на одном — `active`),
    /// условия не повторяются, плейлист берётся у первого, у кого он есть.
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

        // Холст без дисплеев — пусто, а не чужое.
        assert_eq!(canvas_runtime("none", &views), CanvasRuntime::default());
    }

    fn library(id: i64, name: &str, path: &str) -> pb::LibraryInstance {
        pb::LibraryInstance {
            id,
            path: path.to_string(),
            plugin_name: "we".to_string(),
            name: name.to_string(),
        }
    }

    fn event(payload: pb::event::Payload) -> pb::Event {
        pb::Event {
            payload: Some(payload),
        }
    }

    /// waywallen 0.5.0, `ui/src/objmodel/library.cpp`: `initialized`
    /// поднимается первым полным списком и больше не опускается; имя
    /// библиотеки приходит со снимком и с `LibraryChanged`; изменения до
    /// снимка `initialized` не поднимают.
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
                },
                &LibraryInfo {
                    id: 3,
                    name: "Мастерская".into(),
                    path: "/w".into(),
                    plugin_name: "we".into(),
                },
            ],
            "снимок заменяет список целиком, порядок по номеру"
        );

        // Переименование демона приходит как LibraryChanged.
        assert!(lm.apply_event(&event(Payload::LibraryChanged(pb::LibraryChanged {
            library: Some(library(3, "Steam", "/w")),
        }))));
        assert_eq!(lm.by_id[&3].name, "Steam");

        // Пустой снимок: библиотек нет, но список уже известен.
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
