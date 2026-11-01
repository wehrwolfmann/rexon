// Плейлисты обоев: список, подробности одного и мутации (создать,
// удалить, переименовать, задать состав/режим/интервал, обновить целиком,
// включить, выключить, перескочить).
//
// Эталон — waywallen `ui/src/query/playlist_query.cpp`
// @e5626c1 (вендорный коммит v0.2.6+9).
//
// Что изменилось против нашего Python-моста:
//
//   * убраны свойства `data` и `count`. `data` отдавало пустую
//     модель-заглушку (`_EmptyListModel`), `count` — длину списка; ни того,
//     ни другого нет у оригинала, и НИ ОДНА строка QML их не читает
//     (проверено grep'ом по вендорному дереву и по нашим страницам).
//     Держать мёртвую модель ради имени свойства незачем.
//   * `sourceKind` в карте плейлиста ОСТАВЛЕН, хотя у оригинала его нет:
//     по нему наши страницы отличают «умный» плейлист от обычного, а
//     лишний ключ вендорному QML не мешает.
//
// Сигнал `done()` летит ВСЕГДА — и при отказе демона тоже: QML после него
// читает `.status` (так же у оригинала, `PlaylistMutationQuery::send`).

/// Мост cxx-qt: список плейлистов и мутации.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// Создать модель состава плейлиста как ребёнка запроса.
        ///
        /// Имена свои, а не общие с мостом страницы «Обои»: одно и то же
        /// объявление `extern "C++"` в двух мостах одного крейта даёт
        /// `duplicate symbol` на линковке.
        ///
        /// # Safety
        /// `parent` — живой QObject (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_wallpaper_list_model(parent: *mut QObject) -> *mut QObject;

        /// Одноразовый таймер отложенной перезагрузки (`delayReload`
        /// базы `Query`, 100 мс).
        ///
        /// # Safety
        /// `owner` — живой QObject с инвокаблом `reload()`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_debounce(owner: *mut QObject, interval_ms: i32)
            -> *mut QObject;

        /// # Safety
        /// `timer` — таймер, созданный `rexon_new_playlist_debounce`.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_restart_playlist_debounce(timer: *mut QObject);

        // ── Фабрики стенда (src/wallpaper_page_selftest.rs) ──────────────
        //
        // В живом Rexon эти объекты создаёт QML; стенду нужны те же классы
        // без окна и без QML — как у запросов страницы «Обои».
        //
        // # Safety
        // `parent` — либо nullptr, либо живой QObject (владение уходит Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_list_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_detail_query(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_playlist_mutation_query(parent: *mut QObject) -> *mut QObject;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, playlists, READ = playlists, NOTIFY = playlists_changed)]
        type PlaylistListQuery = super::PlaylistListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// ⚠️ `f64`, а не `i64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`, живая проверка —
        /// `src/qml_int64_probe.rs`).
        #[qproperty(f64, created_id, cxx_name = "createdId", READ = created_id,
                    NOTIFY = created_id_changed)]
        type PlaylistMutationQuery = super::PlaylistMutationQueryRust;

        /// Подробности одного плейлиста — страница правки `PlaylistEditPage`
        /// вендора 0.3.7 (она заменила снятую `PlaylistDetailPanel`).
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        /// ⚠️ `double`, а не `qint64`, хотя у оригинала здесь `qint64`.
        /// Причина техническая и проверена живьём: cxx-qt печатает
        /// `Q_PROPERTY(::std::int64_t …)`, а на 64-битном Linux это `long`
        /// — отдельный метатип Qt, для которого движок QML НЕ умеет
        /// декларативную привязку. `playlistId: root.playlistId` в
        /// `PlaylistEditPage.qml` падало бы на загрузке страницы
        /// («Cannot assign value … to property playlistId»), причём
        /// императивное присваивание при этом работает — потому дефект и
        /// не виден ни сборке, ни гейту типов. Сам вендор объявляет это
        /// значение на стороне QML как `required property double
        /// playlistId`, так что расхождения в наблюдаемом поведении нет:
        /// номера плейлистов далеко ниже 2^53.
        #[qproperty(f64, playlist_id, cxx_name = "playlistId", READ = playlist_id,
                    WRITE = set_playlist_id, NOTIFY = playlist_id_changed)]
        #[qproperty(QVariant, playlist, READ = playlist, NOTIFY = playlist_changed)]
        #[qproperty(f64, revision, READ = revision, NOTIFY = playlist_changed)]
        #[qproperty(QStringList, entry_ids, cxx_name = "entryIds", READ = entry_ids,
                    NOTIFY = playlist_changed)]
        /// Модель состава. Имя свойства — `data`, как у оригинала:
        /// вендорный QML пишет `model: detailQuery.data`. CONSTANT —
        /// модель создаётся в пост-конструкции и живёт, пока жив запрос.
        #[qproperty(*mut QObject, model, cxx_name = "data", READ = data, CONSTANT)]
        type PlaylistDetailQuery = super::PlaylistDetailQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistListQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistMutationQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistMutationQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistMutationQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistMutationQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PlaylistDetailQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PlaylistDetailQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PlaylistDetailQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PlaylistDetailQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PlaylistDetailQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "playlistIdChanged"]
        fn playlist_id_changed(self: Pin<&mut PlaylistDetailQuery>);
        /// Один сигнал на карту, номер правки и состав — как у оригинала.
        #[qsignal]
        #[cxx_name = "playlistChanged"]
        fn playlist_changed(self: Pin<&mut PlaylistDetailQuery>);

        #[qsignal]
        #[cxx_name = "playlistsChanged"]
        fn playlists_changed(self: Pin<&mut PlaylistListQuery>);
        #[qsignal]
        #[cxx_name = "createdIdChanged"]
        fn created_id_changed(self: Pin<&mut PlaylistMutationQuery>);
        /// «Мутация доиграла» — успех это или отказ, читается из `status`.
        #[qsignal]
        fn done(self: Pin<&mut PlaylistMutationQuery>);
    }

    extern "RustQt" {
        fn status(self: &PlaylistListQuery) -> i32;
        fn error(self: &PlaylistListQuery) -> QString;
        fn querying(self: &PlaylistListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistListQuery>, value: bool);

        fn status(self: &PlaylistMutationQuery) -> i32;
        fn error(self: &PlaylistMutationQuery) -> QString;
        fn querying(self: &PlaylistMutationQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistMutationQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistMutationQuery>, value: bool);

        fn playlists(self: &PlaylistListQuery) -> QVariant;
        #[cxx_name = "createdId"]
        fn created_id(self: &PlaylistMutationQuery) -> f64;

        fn status(self: &PlaylistDetailQuery) -> i32;
        fn error(self: &PlaylistDetailQuery) -> QString;
        fn querying(self: &PlaylistDetailQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PlaylistDetailQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PlaylistDetailQuery>, value: bool);
        #[cxx_name = "playlistId"]
        fn playlist_id(self: &PlaylistDetailQuery) -> f64;
        #[cxx_name = "setPlaylistId"]
        fn set_playlist_id(self: Pin<&mut PlaylistDetailQuery>, value: f64);
        fn playlist(self: &PlaylistDetailQuery) -> QVariant;
        fn revision(self: &PlaylistDetailQuery) -> f64;
        #[cxx_name = "entryIds"]
        fn entry_ids(self: &PlaylistDetailQuery) -> QStringList;
    }

    // unsafe-блок: геттер свойства `data` отдаёт голый указатель.
    unsafe extern "RustQt" {
        fn data(self: &PlaylistDetailQuery) -> *mut QObject;
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistListQuery>, token: i64, frame: &QByteArray);

        /// У оригинала `reload()` мутации — пустой: она запускается только
        /// явным вызовом одного из действий ниже.
        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistMutationQuery>);
        /// ⚠️ Сигнатура 0.3.7: `synchronizedSelection` встал ЧЕТВЁРТЫМ,
        /// перед составом (`PlaylistMutationQuery::create` оригинала).
        #[qinvokable]
        fn create(self: Pin<&mut PlaylistMutationQuery>, name: QString, mode: i32,
                  interval_secs: i32, synchronized_selection: bool, item_ids: QVariant);

        /// Обновить плейлист целиком — форма правки 0.3.7.
        #[qinvokable]
        fn update(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString, mode: i32,
                  interval_secs: i32, synchronized_selection: bool, item_ids: QVariant,
                  expected_revision: f64);
        /// Сохранить умный плейлист (waywallen 0.5.0): `id > 0` — обновить,
        /// иначе создать. `selection` — непрозрачная выборка
        /// `WallpaperListQuery.selection`.
        #[qinvokable]
        #[cxx_name = "saveSmart"]
        fn save_smart(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString, mode: i32,
                      interval_secs: i32, synchronized_selection: bool, selection: &QVariant,
                      expected_revision: f64);
        #[qinvokable]
        fn remove(self: Pin<&mut PlaylistMutationQuery>, id: f64);
        #[qinvokable]
        fn rename(self: Pin<&mut PlaylistMutationQuery>, id: f64, name: QString);
        /// ⚠️ Сигнатура 0.3.7: третьим параметром — номер правки, на
        /// которой состав читали. Демон сверяет его со своим и отвечает
        /// `ERROR_CODE_PLAYLIST_REVISION_CONFLICT`, если между чтением и
        /// записью плейлист успел измениться.
        #[qinvokable]
        #[cxx_name = "setItems"]
        fn set_items(self: Pin<&mut PlaylistMutationQuery>, id: f64, item_ids: QVariant,
                     expected_revision: f64);
        #[qinvokable]
        #[cxx_name = "setMode"]
        fn set_mode(self: Pin<&mut PlaylistMutationQuery>, id: f64, mode: i32);
        #[qinvokable]
        #[cxx_name = "setInterval"]
        fn set_interval(self: Pin<&mut PlaylistMutationQuery>, id: f64, interval_secs: i32);
        #[qinvokable]
        fn activate(self: Pin<&mut PlaylistMutationQuery>, id: f64, targets: QVariant,
                    auto_attach: bool);
        #[qinvokable]
        fn deactivate(self: Pin<&mut PlaylistMutationQuery>, targets: QVariant,
                      clear_auto_attach: f64);
        #[qinvokable]
        #[cxx_name = "jumpTo"]
        fn jump_to(self: Pin<&mut PlaylistMutationQuery>, id: f64, entry_id: QString);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistMutationQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PlaylistDetailQuery>);
        /// `delayReload` базы `Query` (qextra): перезагрузка через 100 мс,
        /// подряд идущие поводы склеиваются в ОДИН запрос. Из вендорного
        /// QML её зовёт только страница правки плейлиста — она же
        /// перечитывает состав после каждой мутации и после события
        /// демона.
        #[qinvokable]
        #[cxx_name = "delayReload"]
        fn delay_reload(self: Pin<&mut PlaylistDetailQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PlaylistDetailQuery>, token: i64,
                            frame: &QByteArray);
    }

    // Пост-конструкция: модель состава и таймер отложенной перезагрузки.
    impl cxx_qt::Constructor<()> for PlaylistDetailQuery {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_manage;
use rexon_core::we_query::{QuerySignal, QueryState};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::qvalue::{json_to_variant, variant_to_json};

/// Плейлист ядра → карта QML. Выборку умного плейлиста (waywallen 0.5.0)
/// кладём под `smartSelection` байтами — тем же непрозрачным значением,
/// что `WallpaperListQuery.selection`: `PlaylistEditPage` отдаёт его прямо
/// в `restoreSelection`/`matchesSelection`. В JSON карты ядра байтам места
/// нет, поэтому ключ дописывается здесь.
fn playlist_variant(view: &we_manage::PlaylistView) -> QVariant {
    let (Value::Object(map), Some(bytes)) = (&view.map, &view.smart_selection) else {
        return json_to_variant(&view.map);
    };
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, item) in map {
        out.insert(QString::from(key), json_to_variant(item));
    }
    out.insert(
        QString::from(we_manage::SMART_SELECTION_KEY),
        QVariant::from(&QByteArray::from(&bytes[..])),
    );
    QVariant::from(&out)
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistListQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PlaylistListQueryRust {
    base: QueryState,
    data: we_manage::PlaylistListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PlaylistListQuery);

impl qobject::PlaylistListQuery {
    pub fn playlists(&self) -> QVariant {
        let mut list = QList::<QVariant>::default();
        for view in &self.data.playlists {
            list.append(playlist_variant(view));
        }
        QVariant::from(&list)
    }

    /// Новое поколение (`++m_generation` оригинала): ответ на прежний
    /// запрос, если он ещё в полёте, больше не наш.
    pub fn reload(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().pending.clear();
        let payload = pb::PlaylistListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("playlist_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_playlist_list(&mut rust.base, &mut rust.data, frame.as_slice())
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
            "playlists" => self.as_mut().playlists_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistMutationQuery
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PlaylistMutationQueryRust {
    base: QueryState,
    created_id: i64,
    /// token → брать ли из ответа номер созданного плейлиста.
    pending: std::collections::BTreeMap<i64, bool>,
}

impl_query_base!(PlaylistMutationQuery);

impl qobject::PlaylistMutationQuery {
    pub fn created_id(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.created_id)
    }

    /// Отправить мутацию. `capture_create` — ждать ли номер нового
    /// плейлиста в ответе.
    fn send(
        mut self: Pin<&mut Self>,
        field: &'static str,
        payload: Vec<u8>,
        capture_create: bool,
    ) {
        if let Some(token) = self.as_mut().send_frame(field, payload) {
            self.as_mut().rust_mut().pending.insert(token, capture_create);
            return;
        }
        // Соединения нет — кадр не ушёл, но `done()` обязан прилететь:
        // страница ждёт его, чтобы снять «сохраняем…».
        self.done();
    }

    pub fn reload(self: Pin<&mut Self>) {
        let _ = self;
    }

    pub fn create(
        self: Pin<&mut Self>,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        item_ids: QVariant,
    ) {
        let payload = pb::PlaylistCreateRequest {
            name: name.to_string(),
            mode,
            interval_secs: interval_secs.max(0) as u32,
            entry_ids: we_manage::list_to_strings(&variant_to_json(&item_ids)),
            synchronized_selection,
            // waywallen 0.5.0: без `source` демон создаёт курируемый
            // плейлист из `entry_ids` — то, что и делает эта форма.
            source: None,
        }
        .encode_to_vec();
        self.send("playlist_create", payload, true);
    }

    /// `PlaylistUpdate` (поле 87 протокола 0.3.7): имя, режим, интервал,
    /// синхронный выбор и состав одним кадром — так их правит форма
    /// `PlaylistEditPage`. `expected_revision` — сторож гонки правок.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        self: Pin<&mut Self>,
        id: f64,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        item_ids: QVariant,
        expected_revision: f64,
    ) {
        // Умному плейлисту (waywallen 0.5.0) ядро подставит его выборку
        // вместо состава — см. `we_manage::playlist_update_request`.
        let payload = we_manage::playlist_update_request(
            crate::qvalue::qml_to_i64(id),
            &name.to_string(),
            mode,
            interval_secs.max(0) as u32,
            synchronized_selection,
            we_manage::list_to_strings(&variant_to_json(&item_ids)),
            crate::qvalue::qml_to_i64(expected_revision),
        )
        .encode_to_vec();
        self.send("playlist_update", payload, false);
    }

    /// `saveSmart` оригинала (waywallen 0.5.0). Кадр собирает ядро
    /// (`we_manage::playlist_save_smart_request`): выборка очищена, состав
    /// пустой. Не выборка — кадра нет, ошибка и `done()`, как у оригинала.
    #[allow(clippy::too_many_arguments)]
    pub fn save_smart(
        mut self: Pin<&mut Self>,
        id: f64,
        name: QString,
        mode: i32,
        interval_secs: i32,
        synchronized_selection: bool,
        selection: &QVariant,
        expected_revision: f64,
    ) {
        let save = crate::wallpaper_queries::selection_bytes(selection).and_then(|bytes| {
            we_manage::playlist_save_smart_request(
                crate::qvalue::qml_to_i64(id),
                &name.to_string(),
                mode,
                interval_secs.max(0) as u32,
                synchronized_selection,
                &bytes,
                crate::qvalue::qml_to_i64(expected_revision),
            )
        });
        let Some(save) = save else {
            let signals = we_manage::playlist_smart_invalid(&mut self.as_mut().rust_mut().base);
            self.dispatch(signals);
            return;
        };
        self.send(save.field(), save.encode_to_vec(), save.captures_create());
    }

    pub fn remove(self: Pin<&mut Self>, id: f64) {
        let id = crate::qvalue::qml_to_i64(id);
        let payload = pb::PlaylistDeleteRequest { id }.encode_to_vec();
        self.send("playlist_delete", payload, false);
    }

    pub fn rename(self: Pin<&mut Self>, id: f64, name: QString) {
        let payload = pb::PlaylistRenameRequest {
            id: crate::qvalue::qml_to_i64(id),
            name: name.to_string(),
        }
        .encode_to_vec();
        self.send("playlist_rename", payload, false);
    }

    /// `expected_revision` — номер правки, на которой состав читали. Ноль
    /// демон трактует как «не сверять».
    pub fn set_items(
        self: Pin<&mut Self>,
        id: f64,
        item_ids: QVariant,
        expected_revision: f64,
    ) {
        let payload = pb::PlaylistSetItemsRequest {
            id: crate::qvalue::qml_to_i64(id),
            entry_ids: we_manage::list_to_strings(&variant_to_json(&item_ids)),
            expected_revision: crate::qvalue::qml_to_i64(expected_revision),
        }
        .encode_to_vec();
        self.send("playlist_set_items", payload, false);
    }

    pub fn set_mode(self: Pin<&mut Self>, id: f64, mode: i32) {
        let id = crate::qvalue::qml_to_i64(id);
        let payload = pb::PlaylistSetModeRequest { id, mode }.encode_to_vec();
        self.send("playlist_set_mode", payload, false);
    }

    pub fn set_interval(self: Pin<&mut Self>, id: f64, interval_secs: i32) {
        let payload = pb::PlaylistSetIntervalRequest {
            id: crate::qvalue::qml_to_i64(id),
            interval_secs: interval_secs.max(0) as u32,
        }
        .encode_to_vec();
        self.send("playlist_set_interval", payload, false);
    }

    /// `targets` — цели v0.3.5: список карт с `displayId` либо `canvasId`
    /// (`WallpaperPage.qml:854`). Пустой список демон трактует как «каждый
    /// холст по разу плюс каждый самостоятельный дисплей».
    pub fn activate(self: Pin<&mut Self>, id: f64, targets: QVariant, auto_attach: bool) {
        let payload = pb::PlaylistActivateRequest {
            id: crate::qvalue::qml_to_i64(id),
            targets: we_manage::presentation_targets(&variant_to_json(&targets)),
            auto_attach,
        }
        .encode_to_vec();
        self.send("playlist_activate", payload, false);
    }

    pub fn deactivate(self: Pin<&mut Self>, targets: QVariant, clear_auto_attach: f64) {
        let payload = pb::PlaylistDeactivateRequest {
            targets: we_manage::presentation_targets(&variant_to_json(&targets)),
            clear_auto_attach: crate::qvalue::qml_to_i64(clear_auto_attach),
        }
        .encode_to_vec();
        self.send("playlist_deactivate", payload, false);
    }

    /// ⚠️ Демон v0.2.4 выполняет прыжок, но НЕ рассылает `playlist_changed`
    /// — push-статусы дисплеев остаются старыми. Проверено живьём
    /// 2026-07-12, кандидат в патч демону; со стороны UI лечить нечем.
    pub fn jump_to(self: Pin<&mut Self>, id: f64, entry_id: QString) {
        let payload = pb::PlaylistJumpToRequest {
            id: crate::qvalue::qml_to_i64(id),
            entry_id: entry_id.to_string(),
        }
        .encode_to_vec();
        self.send("playlist_jump_to", payload, false);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        let Some(capture_create) = self.as_mut().rust_mut().pending.remove(&token) else {
            return;
        };
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_playlist_mutation(
                &mut rust.base,
                &mut rust.created_id,
                capture_create,
                frame.as_slice(),
            )
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => {
                let mut signals = decode_failed(&mut self.as_mut().rust_mut().base);
                signals.push(QuerySignal::Property("done"));
                self.dispatch(signals);
            }
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "createdId" => self.as_mut().created_id_changed(),
            "done" => self.as_mut().done(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PlaylistDetailQuery
// ════════════════════════════════════════════════════════════════════════

/// Дебаунс перезагрузки — 100 мс, как у базы `Query` (qextra query.cpp:16).
const DETAIL_DEBOUNCE_MS: i32 = 100;

pub struct PlaylistDetailQueryRust {
    base: QueryState,
    data: we_manage::PlaylistDetailData,
    playlist_id: i64,
    /// Токены отправленных кадров: ответ на устаревший запрос не должен
    /// перебивать свежий.
    pending: std::collections::BTreeSet<i64>,
    /// Модель состава — свойство `data`, ставится в пост-конструкции.
    model: *mut qobject::QObject,
    debounce: *mut qobject::QObject,
}

impl Default for PlaylistDetailQueryRust {
    fn default() -> Self {
        Self {
            base: QueryState::default(),
            data: we_manage::PlaylistDetailData::default(),
            playlist_id: 0,
            pending: std::collections::BTreeSet::new(),
            model: core::ptr::null_mut(),
            debounce: core::ptr::null_mut(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlaylistDetailQuery {
    fn initialize(mut self: Pin<&mut Self>) {
        let owner: *mut qobject::PlaylistDetailQuery =
            unsafe { self.as_mut().get_unchecked_mut() };
        let owner = owner.cast::<qobject::QObject>();
        // Модель создаётся в пост-конструкции, а не лениво в геттере: QML
        // читает `data` при первой же привязке.
        let model = unsafe { qobject::rexon_new_playlist_wallpaper_list_model(owner) };
        let timer = unsafe { qobject::rexon_new_playlist_debounce(owner, DETAIL_DEBOUNCE_MS) };
        let mut rust = self.as_mut().rust_mut();
        rust.model = model;
        rust.debounce = timer;
    }
}

impl_query_base!(PlaylistDetailQuery);

impl qobject::PlaylistDetailQuery {
    pub fn data(&self) -> *mut qobject::QObject {
        self.model
    }

    pub fn playlist_id(&self) -> f64 {
        self.playlist_id as f64
    }

    /// У оригинала на `playlistIdChanged` висит `delayReload`
    /// (`connect_requet_reload` в `componentComplete`). Отдельного
    /// «доигрывания при создании» нам не нужно: QML присваивает свойство
    /// привязкой, и первая же запись заводит тот же таймер.
    pub fn set_playlist_id(mut self: Pin<&mut Self>, value: f64) {
        // Дробную часть отбрасываем: номер плейлиста целый, а QML
        // присваивает своё `double`.
        let value = value as i64;
        if self.playlist_id == value {
            return;
        }
        // Новое поколение и отмена (`setPlaylistId` оригинала): ответ про
        // прежний плейлист, ещё летящий, новый не перебьёт.
        let mut rust = self.as_mut().rust_mut();
        rust.playlist_id = value;
        rust.pending.clear();
        self.as_mut().playlist_id_changed();
        self.delay_reload();
    }

    pub fn playlist(&self) -> QVariant {
        playlist_variant(&self.data.playlist)
    }

    pub fn revision(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.data.revision)
    }

    pub fn entry_ids(&self) -> QStringList {
        QStringList::from_iter(self.data.entry_ids.iter().map(QString::from))
    }

    pub fn delay_reload(self: Pin<&mut Self>) {
        let timer = self.debounce;
        if timer.is_null() {
            // Таймера нет только в голом Rust-стенде без Qt — там
            // перезагрузка идёт сразу, как у оригинала при `delay: false`.
            self.reload();
            return;
        }
        unsafe { qobject::rexon_restart_playlist_debounce(timer) };
    }

    /// Новое поколение на каждый вызов, как у оригинала. Без номера —
    /// МОЛЧА: ни кадра, ни ошибки, ни смены статуса (waywallen 0.5.0,
    /// `playlist_query.cpp`). Страница создания умного плейлиста живёт с
    /// `playlistId: 0`, и ошибка здесь показала бы «Playlist unavailable».
    pub fn reload(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().pending.clear();
        let id = self.playlist_id;
        if id <= 0 {
            return;
        }
        // waywallen 0.5.0: страницы состава. Ноль в `page_size` — ответ
        // целиком, как до появления полей.
        let payload = pb::PlaylistGetRequest {
            id,
            page: 0,
            page_size: 0,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("playlist_get", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_playlist_get(&mut rust.base, &mut rust.data, frame.as_slice())
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
            "items" => {
                // Модель перестраивается ПЕРВОЙ: страница пересобирает
                // черновик по `playlistChanged` и обязана увидеть уже
                // новый состав.
                let rows = self.data.items.clone();
                if !self.model.is_null() {
                    let model: Pin<&mut crate::wallpaper_list_model::qobject::WallpaperListModel> =
                        unsafe {
                            Pin::new_unchecked(
                                &mut *self.model.cast::<
                                    crate::wallpaper_list_model::qobject::WallpaperListModel,
                                >(),
                            )
                        };
                    model.reset_rows(rows);
                }
            }
            "playlist" => self.as_mut().playlist_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
