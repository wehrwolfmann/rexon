// WallpaperSelectStorage — множественный выбор обоей на странице «Обои».
//
// ⚠️ Эталон — ОРИГИНАЛ, а не наш Python. Питоновский класс
// (bridge.py:4459) сам себя называет «упрощённая, этап 4a» и упрощён
// настолько, что переносить его означало бы законсервировать четыре
// расхождения (правило #16 и правило «во вкладке чужой программы мерило —
// паритет с оригиналом»):
//
//   1. выбор вёлся по ИНДЕКСАМ строк, а у оригинала — по ключам, и живёт он
//      в модели (`qextra/src/select_storage.cpp` + `kstore` QMetaListModel);
//   2. `removableSelectedCount`/`removableSelectedWallpaperIds` отдавали
//      ВЕСЬ выбор, не глядя на `supportsItemRemove`
//      (`ui/src/model/wallpaper_select_storage.cpp:65`), — то есть кнопка
//      «Удалить» обещала удалить то, что удалить нельзя;
//   3. `isEditingPlaylist` всегда возвращал false, поэтому режим правки
//      плейлиста в интерфейсе не отмечался никак;
//   4. `editPlaylistSelection` только запоминал плейлист, а обязан ещё и
//      наполнить выбор его составом, включить режим выбора и сбросить якорь.
//
// Наследования у нас нет (cxx-qt не умеет Rust-QObject от Rust-QObject),
// поэтому база `SelectStorage` перенесена СОСТАВОМ: её поля и правила лежат
// в чистой структуре `SelectState`, а тонкости обоей — рядом. Ровно тот же
// приём, что с базой `Query` в партии 3.
//
// Само выделение хранит МОДЕЛЬ (crate::wallpaper_list_model), как у
// оригинала. Здесь — только режим выбора, якорь, признак активности и
// плейлист, который сейчас правят.

/// Мост cxx-qt: хранилище выбора страницы «Обои».
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    // ⚠️ `type QObject` здесь НЕ объявляется. У этого моста QObject без
    // базы, и cxx-qt печатает объявление сам (cxx-qt-gen: add_qobject_import);
    // второе даёт «the type QObject is defined multiple times». В мостах
    // моделей всё наоборот — там база объявлена вручную, и объявление QObject
    // приходится писать самим.
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Проверенное приведение к модели библиотеки обоев.
        ///
        /// Именно `qobject_cast`, а не голое приведение указателя: свойство
        /// `model` пишет QML, и подсунуть туда можно ЛЮБОЙ QObject. У
        /// оригинала здесь тоже `qobject_cast` — чужая модель просто не
        /// принимается (`select_storage.cpp:16`).
        ///
        /// # Safety
        /// `object` — либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_as_wallpaper_list_model(object: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(*mut QObject, model, READ = model, WRITE = set_model,
                    NOTIFY = model_changed)]
        #[qproperty(bool, selection_mode, cxx_name = "selectionMode",
                    READ = selection_mode, WRITE = set_selection_mode,
                    NOTIFY = selection_mode_changed)]
        #[qproperty(i32, anchor_index, cxx_name = "anchorIndex",
                    READ = anchor_index, WRITE = set_anchor_index,
                    NOTIFY = anchor_index_changed)]
        #[qproperty(i32, selected_count, cxx_name = "selectedCount",
                    READ = selected_count, NOTIFY = selected_count_changed)]
        #[qproperty(bool, active, READ = active, NOTIFY = active_changed)]
        #[qproperty(i32, removable_selected_count, cxx_name = "removableSelectedCount",
                    READ = removable_selected_count,
                    NOTIFY = selected_removable_count_changed)]
        /// waywallen 0.4.4: сколько выбранных обоев скрыто — по нему
        /// страница решает, «Скрыть» или «Вернуть» выбранное.
        #[qproperty(i32, hidden_selected_count, cxx_name = "hiddenSelectedCount",
                    READ = hidden_selected_count,
                    NOTIFY = selected_hidden_count_changed)]
        #[qproperty(QVariant, playlist_edit_target, cxx_name = "playlistEditTarget",
                    READ = playlist_edit_target, WRITE = set_playlist_edit_target,
                    NOTIFY = playlist_edit_target_changed)]
        /// ⚠️ `f64`, а не `i64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`). Поле внутри остаётся точным `i64`.
        #[qproperty(f64, playlist_edit_target_id, cxx_name = "playlistEditTargetId",
                    READ = playlist_edit_target_id,
                    NOTIFY = playlist_edit_target_changed)]
        type WallpaperSelectStorage = super::WallpaperSelectStorageRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "modelChanged"]
        fn model_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectionModeChanged"]
        fn selection_mode_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "anchorIndexChanged"]
        fn anchor_index_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "activeChanged"]
        fn active_changed(self: Pin<&mut WallpaperSelectStorage>);
        /// Отдельный сигнал оригинала: удаляемых может стать меньше без
        /// изменения размера выбора (сменился список).
        #[qsignal]
        #[cxx_name = "selectedRemovableCountChanged"]
        fn selected_removable_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        /// waywallen 0.4.4: скрытых в выборе стало другое число —
        /// сменился выбор, модель или признак «скрыта» у строки.
        #[qsignal]
        #[cxx_name = "selectedHiddenCountChanged"]
        fn selected_hidden_count_changed(self: Pin<&mut WallpaperSelectStorage>);
        #[qsignal]
        #[cxx_name = "playlistEditTargetChanged"]
        fn playlist_edit_target_changed(self: Pin<&mut WallpaperSelectStorage>);
    }

    // unsafe-блок: аксессоры свойства `model` берут голый указатель, и cxx
    // требует помечать такие функции unsafe (речь про соответствие
    // C++-сигнатуре, а не про опасность вызова).
    // ⚠️ `set_model` — сверх того `unsafe fn` сама по себе: она ПЕРЕДАЁТ
    // указатель в шим `qobject_cast`, а тот его разыменовывает. Указатель
    // приходит из QML, за его живость отвечает вызывающая сторона.
    unsafe extern "RustQt" {
        fn model(self: &WallpaperSelectStorage) -> *mut QObject;
        #[cxx_name = "setModel"]
        unsafe fn set_model(self: Pin<&mut WallpaperSelectStorage>, model: *mut QObject);
        #[cxx_name = "selectionMode"]
        fn selection_mode(self: &WallpaperSelectStorage) -> bool;
        #[cxx_name = "setSelectionMode"]
        fn set_selection_mode(self: Pin<&mut WallpaperSelectStorage>, value: bool);
        #[cxx_name = "anchorIndex"]
        fn anchor_index(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "setAnchorIndex"]
        fn set_anchor_index(self: Pin<&mut WallpaperSelectStorage>, value: i32);
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &WallpaperSelectStorage) -> i32;
        fn active(self: &WallpaperSelectStorage) -> bool;
        #[cxx_name = "removableSelectedCount"]
        fn removable_selected_count(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "hiddenSelectedCount"]
        fn hidden_selected_count(self: &WallpaperSelectStorage) -> i32;
        #[cxx_name = "playlistEditTarget"]
        fn playlist_edit_target(self: &WallpaperSelectStorage) -> QVariant;
        #[cxx_name = "setPlaylistEditTarget"]
        fn set_playlist_edit_target(self: Pin<&mut WallpaperSelectStorage>, value: QVariant);
        #[cxx_name = "playlistEditTargetId"]
        fn playlist_edit_target_id(self: &WallpaperSelectStorage) -> f64;
    }

    // ── Инвокаблы (имена — из оригинала) ─────────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn begin(self: Pin<&mut WallpaperSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(self: Pin<&mut WallpaperSelectStorage>, row: i32, selected: bool) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut WallpaperSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut WallpaperSelectStorage>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        fn clear(self: Pin<&mut WallpaperSelectStorage>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut WallpaperSelectStorage>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &WallpaperSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &WallpaperSelectStorage) -> QList_QVariant;

        /// Выбранные идентификаторы списком значений — так их принимает
        /// плейлист-действие вендорной страницы.
        #[qinvokable]
        #[cxx_name = "selectedWallpaperIds"]
        fn selected_wallpaper_ids(self: &WallpaperSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "removableSelectedWallpaperIds"]
        fn removable_selected_wallpaper_ids(self: &WallpaperSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "hasPlaylistEditTarget"]
        fn has_playlist_edit_target(self: &WallpaperSelectStorage) -> bool;

        #[qinvokable]
        #[cxx_name = "isEditingPlaylist"]
        fn is_editing_playlist(self: &WallpaperSelectStorage, playlist: &QVariant) -> bool;

        #[qinvokable]
        #[cxx_name = "editPlaylistSelection"]
        fn edit_playlist_selection(self: Pin<&mut WallpaperSelectStorage>, playlist: &QVariant);

        /// Пересчитать всё, что зависит от выбора в модели. Зовётся, когда
        /// модель сообщила о смене выбора (у оригинала это лямбда в
        /// `SelectStorage::connectModel`).
        #[qinvokable]
        #[cxx_name = "onModelSelectionChanged"]
        fn on_model_selection_changed(self: Pin<&mut WallpaperSelectStorage>);
    }

    impl cxx_qt::Constructor<()> for WallpaperSelectStorage {}
}
use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QVariant};
use serde_json::Value;

use crate::impl_select_storage_base;
use crate::qvalue::variant_to_json;

// ============================================================================
// Чистый слой: правила базы SelectStorage без Qt
// ============================================================================

/// Состояние базы `SelectStorage` (qextra) плюс плейлист-правка обоей.
#[derive(Debug, Default)]
pub struct SelectState {
    pub selection_mode: bool,
    pub anchor_index: i32,
    /// Кэш `active`: сигнал шлётся только при РЕАЛЬНОМ перевороте, как у
    /// оригинала (`notifyActiveChanged`).
    pub active: bool,
    pub playlist_target: Value,
    pub playlist_target_id: i64,
}

impl SelectState {
    pub fn new() -> Self {
        Self {
            selection_mode: false,
            anchor_index: -1,
            active: false,
            playlist_target: Value::Null,
            playlist_target_id: 0,
        }
    }

    /// `keepActiveWithoutSelection` оригинала: пока правят плейлист, режим
    /// выбора остаётся активным даже с пустым выбором.
    pub fn keep_active_without_selection(&self) -> bool {
        self.playlist_target_id > 0
    }
}

/// Идентификатор плейлиста из карты, пришедшей из QML.
pub fn playlist_id(value: &Value) -> i64 {
    match value.get("id") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

/// Состав плейлиста — им наполняется выбор при входе в правку.
pub fn playlist_entry_ids(value: &Value) -> Vec<String> {
    match value.get("entryIds") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

// ============================================================================
// Qt-слой
// ============================================================================

/// Rust-состояние хранилища.
pub struct WallpaperSelectStorageRust {
    state: SelectState,
    /// Модель библиотеки. Голый указатель, как у оригинала QPointer:
    /// владелец модели — запрос списка, хранилище её только читает.
    model: *mut qobject::QObject,
    /// Подписки на модель (waywallen 0.4.4, см. `watch_model`): сторожа
    /// рвут связь при смене модели и при уничтожении хранилища.
    model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>,
}

impl Default for WallpaperSelectStorageRust {
    fn default() -> Self {
        Self {
            state: SelectState::new(),
            model: core::ptr::null_mut(),
            model_watch: Vec::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperSelectStorage {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

// База `SelectStorage` + надстройка обоей — общим движком
// (src/select_storage_base.rs), как у оригинала двумя базовыми классами.
impl_select_storage_base!(WallpaperSelectStorage, rexon_as_wallpaper_list_model);

impl qobject::WallpaperSelectStorage {
    // ── крючки общего движка ──────────────────────────────────────────────

    /// `keepActiveWithoutSelection` этого уровня: правка плейлиста.
    fn keep_active(&self) -> bool {
        self.state.keep_active_without_selection()
    }

    /// Своего обработчика `selectedCountChanged` у этого уровня нет: оба
    /// счётчика рассылает общий движок.
    fn after_selected_count_changed(self: Pin<&mut Self>) {
        let _ = self;
    }

    // ── свойства уровня «правка плейлиста» ────────────────────────────────

    fn playlist_edit_target(&self) -> QVariant {
        crate::qvalue::json_to_variant(&self.state.playlist_target)
    }

    fn set_playlist_edit_target(mut self: Pin<&mut Self>, value: QVariant) {
        let target = variant_to_json(&value);
        let next_id = playlist_id(&target);
        if self.state.playlist_target == target && self.state.playlist_target_id == next_id {
            return;
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.state.playlist_target = target;
            rust.state.playlist_target_id = next_id;
        }
        self.as_mut().playlist_edit_target_changed();
        self.notify_active_changed();
    }

    fn playlist_edit_target_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.state.playlist_target_id)
    }

    // ── инвокаблы уровня ──────────────────────────────────────────────────

    /// Порядок как у оригинала: сначала снимается плейлист-правка (иначе
    /// `keepActiveWithoutSelection` не даст выйти из режима), потом выбор.
    fn clear(mut self: Pin<&mut Self>) {
        if self.state.keep_active_without_selection() {
            {
                let mut rust = self.as_mut().rust_mut();
                rust.state.playlist_target = Value::Null;
                rust.state.playlist_target_id = 0;
            }
            self.as_mut().playlist_edit_target_changed();
        }
        self.clear_base();
    }

    fn has_playlist_edit_target(&self) -> bool {
        self.state.playlist_target_id > 0
    }

    fn is_editing_playlist(&self, playlist: &QVariant) -> bool {
        let id = playlist_id(&variant_to_json(playlist));
        id > 0 && id == self.state.playlist_target_id
    }

    fn edit_playlist_selection(mut self: Pin<&mut Self>, playlist: &QVariant) {
        let target = variant_to_json(playlist);
        let entries = playlist_entry_ids(&target);
        self.as_mut()
            .set_playlist_edit_target(crate::qvalue::json_to_variant(&target));
        let keys = QStringList::from_iter(entries.iter().map(QString::from));
        self.as_mut().set_selected_keys(&keys);
        self.as_mut().set_selection_mode(true);
        self.set_anchor_index(-1);
    }
}
