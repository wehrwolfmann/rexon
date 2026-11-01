// PlaylistItemSelectStorage — выбор состава плейлиста на странице «Обои».
//
// Появился у вендора в v0.3.7 (`ui/src/model/wallpaper_select_storage.cppm`)
// и объявлен ПРЯМО в `WallpaperPage.qml:24`: без этого типа страница
// «Обои» не создаётся вовсе, а не «работает с оговорками».
//
// Чем он отличается от `WallpaperSelectStorage`, поверх которого стоит:
//
//   * помнит, состав КАКОГО плейлиста сейчас правят (`playlistId`) и на
//     какой правке (`revision`) он был прочитан — этот номер уходит
//     демону в `expected_revision`, и по нему демон ловит гонку двух
//     правок (`ERROR_CODE_PLAYLIST_REVISION_CONFLICT` протокола 0.3.7);
//   * держит ИСХОДНЫЙ порядок состава (`initialEntryIds`) и порядок
//     добавленного (`m_new_entry_ids` оригинала), поэтому
//     `orderedWallpaperIds()` отдаёт не «что выбрано», а «что выбрано, в
//     том порядке, в каком плейлист сложился»: сначала уцелевшие старые,
//     потом добавленные в порядке добавления;
//   * пока плейлист задан, режим выбора не гаснет на пустом выборе —
//     плейлист можно опустошить целиком и нажать «Применить».
//
// Наследования у cxx-qt нет, поэтому обе базы приходят общим движком
// `impl_select_storage_base!` (src/select_storage_base.rs) — тем же
// приёмом, что база `Query` у запросов.

/// Мост cxx-qt: хранилище выбора состава плейлиста.
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

    // ⚠️ `type QObject` здесь НЕ объявляется — cxx-qt печатает объявление
    // сам (см. пояснение в src/wallpaper_select_storage.rs).
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Проверенное приведение к модели библиотеки обоев.
        ///
        /// Имя своё, а не общее с мостом соседнего хранилища: одно и то же
        /// объявление `extern "C++"` в двух мостах одного крейта даёт
        /// `duplicate symbol` на линковке.
        ///
        /// # Safety
        /// `object` — либо nullptr, либо живой QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_as_playlist_wallpaper_list_model(object: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── база SelectStorage + надстройка обоей ─────────────────────────
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
        // ── свой уровень: правка состава плейлиста ────────────────────────
        /// ⚠️ Оба номера уходят наружу `f64`, а не `i64`: 64-битное целое в
        /// QML не выпускаем — литеральная привязка в такое свойство
        /// отказывает вместе со всем документом. Разбор и границы точности
        /// — `src/qvalue.rs`, живая проверка — `src/qml_int64_probe.rs`.
        /// Внутри поля остаются точными `i64`.
        #[qproperty(f64, playlist_id, cxx_name = "playlistId", READ = playlist_id,
                    NOTIFY = playlist_changed)]
        #[qproperty(f64, revision, READ = revision, NOTIFY = playlist_changed)]
        #[qproperty(QStringList, initial_entry_ids, cxx_name = "initialEntryIds",
                    READ = initial_entry_ids, NOTIFY = playlist_changed)]
        type PlaylistItemSelectStorage = super::PlaylistItemSelectStorageRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "modelChanged"]
        fn model_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectionModeChanged"]
        fn selection_mode_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "anchorIndexChanged"]
        fn anchor_index_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        #[qsignal]
        #[cxx_name = "activeChanged"]
        fn active_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// Отдельный сигнал оригинала: удаляемых может стать меньше без
        /// изменения размера выбора (сменился список).
        #[qsignal]
        #[cxx_name = "selectedRemovableCountChanged"]
        fn selected_removable_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// waywallen 0.4.4: скрытых в выборе стало другое число —
        /// сменился выбор, модель или признак «скрыта» у строки.
        #[qsignal]
        #[cxx_name = "selectedHiddenCountChanged"]
        fn selected_hidden_count_changed(self: Pin<&mut PlaylistItemSelectStorage>);
        /// Сменился правимый плейлист: номер, правка и исходный состав.
        #[qsignal]
        #[cxx_name = "playlistChanged"]
        fn playlist_changed(self: Pin<&mut PlaylistItemSelectStorage>);
    }

    // unsafe-блок: аксессоры свойства `model` берут голый указатель.
    // ⚠️ `set_model` — сверх того `unsafe fn` сама по себе: она ПЕРЕДАЁТ
    // указатель в шим `qobject_cast`, а тот его разыменовывает. Указатель
    // приходит из QML, за его живость отвечает вызывающая сторона.
    unsafe extern "RustQt" {
        fn model(self: &PlaylistItemSelectStorage) -> *mut QObject;
        #[cxx_name = "setModel"]
        unsafe fn set_model(self: Pin<&mut PlaylistItemSelectStorage>, model: *mut QObject);
        #[cxx_name = "selectionMode"]
        fn selection_mode(self: &PlaylistItemSelectStorage) -> bool;
        #[cxx_name = "setSelectionMode"]
        fn set_selection_mode(self: Pin<&mut PlaylistItemSelectStorage>, value: bool);
        #[cxx_name = "anchorIndex"]
        fn anchor_index(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "setAnchorIndex"]
        fn set_anchor_index(self: Pin<&mut PlaylistItemSelectStorage>, value: i32);
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &PlaylistItemSelectStorage) -> i32;
        fn active(self: &PlaylistItemSelectStorage) -> bool;
        #[cxx_name = "removableSelectedCount"]
        fn removable_selected_count(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "hiddenSelectedCount"]
        fn hidden_selected_count(self: &PlaylistItemSelectStorage) -> i32;
        #[cxx_name = "playlistId"]
        fn playlist_id(self: &PlaylistItemSelectStorage) -> f64;
        fn revision(self: &PlaylistItemSelectStorage) -> f64;
        #[cxx_name = "initialEntryIds"]
        fn initial_entry_ids(self: &PlaylistItemSelectStorage) -> QStringList;
    }

    // ── Инвокаблы (имена — из оригинала) ─────────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn begin(self: Pin<&mut PlaylistItemSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(
            self: Pin<&mut PlaylistItemSelectStorage>,
            row: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut PlaylistItemSelectStorage>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut PlaylistItemSelectStorage>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        fn clear(self: Pin<&mut PlaylistItemSelectStorage>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut PlaylistItemSelectStorage>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &PlaylistItemSelectStorage) -> QStringList;

        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "selectedWallpaperIds"]
        fn selected_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        #[qinvokable]
        #[cxx_name = "removableSelectedWallpaperIds"]
        fn removable_selected_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QStringList;

        /// Войти в правку состава плейлиста: запомнить номер и правку,
        /// наполнить выбор его составом, включить режим выбора.
        #[qinvokable]
        #[cxx_name = "beginPlaylistItems"]
        fn begin_playlist_items(
            self: Pin<&mut PlaylistItemSelectStorage>,
            playlist_id: f64,
            revision: f64,
            entry_ids: &QStringList,
        );

        /// Выбранное В ПОРЯДКЕ плейлиста — этим списком уходит
        /// `playlist_set_items`.
        #[qinvokable]
        #[cxx_name = "orderedWallpaperIds"]
        fn ordered_wallpaper_ids(self: &PlaylistItemSelectStorage) -> QList_QVariant;

        /// Пересчитать всё, что зависит от выбора в модели.
        #[qinvokable]
        #[cxx_name = "onModelSelectionChanged"]
        fn on_model_selection_changed(self: Pin<&mut PlaylistItemSelectStorage>);
    }

    impl cxx_qt::Constructor<()> for PlaylistItemSelectStorage {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList, QVariant};

use crate::impl_select_storage_base;
use crate::wallpaper_select_storage::SelectState;

/// Rust-состояние хранилища.
pub struct PlaylistItemSelectStorageRust {
    state: SelectState,
    /// Модель библиотеки. Голый указатель, как у оригинала QPointer.
    model: *mut qobject::QObject,
    /// Подписки на модель (waywallen 0.4.4, см. `watch_model`): сторожа
    /// рвут связь при смене модели и при уничтожении хранилища.
    model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>,
    /// Правимый плейлист и его правка на момент чтения.
    playlist_id: i64,
    revision: i64,
    /// Исходный состав плейлиста по порядку, без повторов и пустых.
    initial_entry_ids: Vec<String>,
    /// Добавленные сверх исходного состава — в порядке добавления.
    new_entry_ids: Vec<String>,
}

impl Default for PlaylistItemSelectStorageRust {
    fn default() -> Self {
        Self {
            state: SelectState::new(),
            model: core::ptr::null_mut(),
            model_watch: Vec::new(),
            playlist_id: 0,
            revision: 0,
            initial_entry_ids: Vec::new(),
            new_entry_ids: Vec::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlaylistItemSelectStorage {
    fn initialize(self: Pin<&mut Self>) {
        let _ = self;
    }
}

// Обе базы оригинала — общим движком (src/select_storage_base.rs).
impl_select_storage_base!(PlaylistItemSelectStorage, rexon_as_playlist_wallpaper_list_model);

impl qobject::PlaylistItemSelectStorage {
    // ── крючки общего движка ──────────────────────────────────────────────

    /// `keepActiveWithoutSelection` этого уровня: пока плейлист задан,
    /// режим выбора держится и на пустом выборе — иначе плейлист нельзя
    /// было бы опустошить целиком.
    fn keep_active(&self) -> bool {
        self.playlist_id > 0
    }

    /// Обработчик `selectedCountChanged`, который оригинал вешает в
    /// конструкторе подкласса: пересчитать порядок добавленных.
    fn after_selected_count_changed(self: Pin<&mut Self>) {
        self.sync_new_entry_order();
    }

    // ── свойства уровня ───────────────────────────────────────────────────

    fn playlist_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.playlist_id)
    }

    fn revision(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.revision)
    }

    fn initial_entry_ids(&self) -> QStringList {
        QStringList::from_iter(self.initial_entry_ids.iter().map(QString::from))
    }

    // ── инвокаблы уровня ──────────────────────────────────────────────────

    fn begin_playlist_items(
        mut self: Pin<&mut Self>,
        playlist_id: f64,
        revision: f64,
        entry_ids: &QStringList,
    ) {
        // Порядок как у оригинала: сначала СБРОС базы (старый выбор не
        // должен просочиться в новый плейлист), потом новое состояние.
        self.as_mut().clear_base();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.playlist_id = crate::qvalue::qml_to_i64(playlist_id);
            rust.revision = crate::qvalue::qml_to_i64(revision);
            rust.initial_entry_ids.clear();
            rust.new_entry_ids.clear();
            for entry_id in entry_ids.iter() {
                let entry_id = entry_id.to_string();
                if entry_id.is_empty() || rust.initial_entry_ids.contains(&entry_id) {
                    continue;
                }
                rust.initial_entry_ids.push(entry_id);
            }
        }
        self.as_mut().playlist_changed();
        let keys = self.initial_entry_ids();
        self.as_mut().set_selected_keys(&keys);
        self.as_mut().set_selection_mode(true);
        self.as_mut().set_anchor_index(-1);
        self.notify_active_changed();
    }

    fn ordered_wallpaper_ids(&self) -> qobject::QList_QVariant {
        let selected: Vec<String> = self
            .selected_keys()
            .iter()
            .map(cxx_qt_lib::QString::to_string)
            .collect();
        let mut emitted: Vec<String> = Vec::new();
        // Сначала уцелевшие из исходного состава — в порядке плейлиста.
        for entry_id in &self.initial_entry_ids {
            if !selected.contains(entry_id) || emitted.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        // Затем добавленные — в порядке добавления.
        for entry_id in &self.new_entry_ids {
            if entry_id.is_empty() || emitted.contains(entry_id) {
                continue;
            }
            if !selected.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        // Хвост: выбранное, о котором мы ещё не знаем (модель сменилась
        // между сигналами) — ветка оригинала, а не подстраховка.
        for entry_id in &selected {
            if entry_id.is_empty() || emitted.contains(entry_id) {
                continue;
            }
            emitted.push(entry_id.clone());
        }
        let mut out = qobject::QList_QVariant::default();
        for entry_id in &emitted {
            out.append(QVariant::from(&QString::from(entry_id)));
        }
        out
    }

    /// Порядок как у оригинала: сначала снимается плейлист (иначе
    /// `keepActiveWithoutSelection` не даст выйти из режима), потом выбор.
    fn clear(mut self: Pin<&mut Self>) {
        let had_playlist = self.playlist_id > 0
            || self.revision > 0
            || !self.initial_entry_ids.is_empty();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.playlist_id = 0;
            rust.revision = 0;
            rust.initial_entry_ids.clear();
            rust.new_entry_ids.clear();
        }
        if had_playlist {
            self.as_mut().playlist_changed();
        }
        self.clear_base();
    }

    // ── внутреннее ────────────────────────────────────────────────────────

    /// `syncNewEntryOrder` оригинала: снятое с выбора забывается, а
    /// незнакомое выбранное дописывается в конец — так добавленные
    /// сохраняют порядок добавления, а не порядок в списке библиотеки.
    fn sync_new_entry_order(mut self: Pin<&mut Self>) {
        let selected: Vec<String> = self
            .selected_keys()
            .iter()
            .map(cxx_qt_lib::QString::to_string)
            .collect();
        let mut rust = self.as_mut().rust_mut();
        rust.new_entry_ids.retain(|entry_id| selected.contains(entry_id));
        for entry_id in &selected {
            if entry_id.is_empty()
                || rust.initial_entry_ids.contains(entry_id)
                || rust.new_entry_ids.contains(entry_id)
            {
                continue;
            }
            rust.new_entry_ids.push(entry_id.clone());
        }
    }
}
